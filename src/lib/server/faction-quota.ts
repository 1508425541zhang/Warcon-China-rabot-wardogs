import { eq, sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import { parseIni, getScalar, setScalarInText } from '$lib/config-doc';
import { quotaInput, quotaTeams, planQuota, type QuotaMove } from '$lib/faction-quota-policy';
import type { Player, Status, ConfigDoc, ConfigResult } from '$lib/types';
import type { Env } from './env';
import { siteSettings, factionMovePermits, type ServerRow } from './db/schema';
import { gateway } from './gateway';
import { ApiError } from './http';
import { isOwner, withOwnedTransaction } from './leadership';
import { ACTIONS, readConfig } from './actions';
import type { WardogsClient } from './rcon';
import { writeAudit } from './audit';

const section = '/Script/WDGame.WDGameStateSession';
const joinKey = 'bLockOverpopulatedTeamsConfig';
const configKey = (id: string) => `factionQuota:${id}`;
const runtimeKey = (id: string) => `factionQuotaRuntime:${id}`;
export type QuotaConfig = ReturnType<typeof quotaInput.parse> & {
	waitMatchId: number | null;
	originalJoinLock: string | null;
	preparing?: boolean;
	preparationAt?: number;
};
const defaults: QuotaConfig = {
	revision: 'empty',
	enabled: false,
	limits: { blue: 50, red: 50, green: 0 },
	graceSeconds: 60,
	waitMatchId: null,
	originalJoinLock: null
};
type QuotaEvent = QuotaMove & { at: string; state: string; reason: string; revision: string };
type Runtime = {
	reason: string;
	events: QuotaEvent[];
	lastCheck?: number;
	nativeReady?: boolean;
	counts?: Record<string, number>;
	targets?: Record<string, number>;
};
export async function quotaConfig(env: Env, serverId: string): Promise<QuotaConfig> {
	const [row] = await env.db
		.select()
		.from(siteSettings)
		.where(eq(siteSettings.key, configKey(serverId)));
	return row ? { ...defaults, ...(row.value as QuotaConfig) } : structuredClone(defaults);
}
export async function factionQuotaView(env: Env, serverId: string) {
	const [row] = await env.db
		.select()
		.from(siteSettings)
		.where(eq(siteSettings.key, runtimeKey(serverId)));
	return {
		config: await quotaConfig(env, serverId),
		runtime: (row?.value as Runtime | undefined) ?? {
			reason: '默认关闭；启用后开始检查人数。',
			events: []
		}
	};
}
async function currentMatch(env: Env, id: string) {
	const [match] = await env.db.execute<{ id: number; started_at: Date; map: string }>(
		sql`SELECT id,started_at,map FROM matches WHERE server_id=${id} AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1`
	);
	return match;
}
async function conflicts(env: Env, id: string) {
	const rows = await env.db.execute(
		sql`SELECT 1 FROM faction_lock_rules WHERE server_id=${id} AND enabled=true UNION ALL SELECT 1 FROM skill_balance_rules WHERE server_id=${id} AND enabled=true`
	);
	return rows.length > 0;
}

/** Prepare only the native join-balance switch, through existing ETag/secret-preserving actions. */
export async function saveFactionQuota(
	env: Env,
	server: ServerRow,
	userId: string,
	input: unknown
) {
	const parsed = quotaInput.safeParse(input);
	if (!parsed.success) throw new ApiError(400, parsed.error.issues[0]?.message ?? '配额格式无效。');
	const requested = parsed.data;
	if (requested.enabled && (await conflicts(env, server.id)))
		throw new ApiError(409, '请先关闭“禁止自行换边”和“强弱阵营平衡”，避免反复调队。');
	let previous!: QuotaConfig;
	const revision = randomUUID();
	const storePrepared = async (value: QuotaConfig) => {
		const rows = await env.db.execute(
			sql`UPDATE site_settings SET value=${JSON.stringify(value)}::text::jsonb,updated_at=now() WHERE key=${configKey(server.id)} AND value->>'revision'=${revision} RETURNING key`
		);
		if (!rows.length) throw new ApiError(409, '准备期间设置已改变，请刷新。');
	};
	await env.db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${configKey(server.id)},0))`
		);
		const [row] = await tx
			.select()
			.from(siteSettings)
			.where(eq(siteSettings.key, configKey(server.id)));
		previous = row ? { ...defaults, ...(row.value as QuotaConfig) } : structuredClone(defaults);
		if (previous.revision !== requested.revision)
			throw new ApiError(409, '设置已变化，请刷新后重试。');
		if (previous.preparing && Date.now() - (previous.preparationAt ?? 0) < 120000)
			throw new ApiError(409, '正在准备游戏配置，请稍后重试。');
		const claim = {
			...previous,
			revision,
			enabled: false,
			preparing: true,
			preparationAt: Date.now()
		};
		await tx
			.insert(siteSettings)
			.values({ key: configKey(server.id), value: claim, updatedBy: userId, updatedAt: new Date() })
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value: claim, updatedBy: userId, updatedAt: new Date() }
			});
	});
	let next: QuotaConfig = { ...previous, ...requested, revision, preparing: false };
	try {
		if (requested.enabled || previous.originalJoinLock !== null) {
			const capabilities = (await gateway().run(env, server, 'capabilities', {})) as {
				features: { changeTeam: boolean; configDocument: boolean };
			};
			if (requested.enabled && !capabilities.features?.changeTeam)
				throw new ApiError(409, '服务器不支持官方管理员调队接口，无法启用。');
			if (!capabilities.features?.configDocument)
				throw new ApiError(409, '服务器不支持可写配置文档。');
			const doc = (await gateway().run(env, server, 'config', {})) as ConfigDoc;
			const existing = getScalar(parseIni(doc.text), section, joinKey);
			if (existing === null)
				throw new ApiError(409, '服务器未提供原生阵营加入限制设置，未自动修改。');
			const desired = requested.enabled ? 'false' : previous.originalJoinLock;
			// Restore only a switch still carrying our value; preserve edits made elsewhere.
			if (
				desired !== null &&
				existing.toLowerCase() !== desired.toLowerCase() &&
				(requested.enabled || existing.toLowerCase() === 'false')
			) {
				if (requested.enabled) {
					next.originalJoinLock = previous.originalJoinLock ?? existing;
					next.waitMatchId = Number((await currentMatch(env, server.id))?.id) || null;
				}
				// Persist the original switch before the external write, so a process interruption
				// still leaves enough information to restore it while the rule remains disabled.
				await storePrepared({
					...next,
					enabled: false,
					preparing: true,
					preparationAt: Date.now()
				});
				const result = (await gateway().run(env, server, 'configApply', {
					revision: doc.revision,
					text: setScalarInText(doc.text, section, joinKey, desired)
				})) as ConfigResult;
				if (!result.ok) throw new ApiError(409, '游戏配置未应用，请刷新后重试。');
				const verified = (await gateway().run(env, server, 'config', {})) as ConfigDoc;
				if (
					getScalar(parseIni(verified.text), section, joinKey)?.toLowerCase() !==
					desired.toLowerCase()
				)
					throw new ApiError(409, '游戏配置中的加入限制未改变，可能被启动参数锁定。');
			}
		}
		if (!requested.enabled) {
			next.originalJoinLock = null;
			next.waitMatchId = null;
		}
	} catch (error) {
		// Interrupted preparation must not activate a partially prepared rule.
		next = { ...next, revision, enabled: false, preparing: false };
		await storePrepared(next);
		throw error;
	}
	await storePrepared(next);
	gateway().observeSoon(server.id);
	return next;
}

/** Runs in the observer's serialized server lane; each pass uses a newly saved roster. */
export async function runFactionQuota(
	env: Env,
	server: ServerRow,
	client: WardogsClient,
	input: {
		players: Player[];
		status: Status;
		statusAt: number;
		trusted: boolean;
		boundary: boolean;
		startupAt: number;
		now: Date;
	}
) {
	if (!isOwner()) return;
	const config = await quotaConfig(env, server.id);
	if (!config.enabled || !quotaInput.safeParse(config).success) return;
	const now = input.now.getTime();
	const [stored] = await env.db
		.select()
		.from(siteSettings)
		.where(eq(siteSettings.key, runtimeKey(server.id)));
	const runtime: Runtime = (stored?.value as Runtime) ?? { reason: '', events: [] };
	const persist = async (reason: string) => {
		runtime.reason = reason;
		await withOwnedTransaction(env, (tx) =>
			tx
				.insert(siteSettings)
				.values({ key: runtimeKey(server.id), value: runtime, updatedAt: input.now })
				.onConflictDoUpdate({
					target: siteSettings.key,
					set: { value: runtime, updatedAt: input.now }
				})
		);
	};
	if (
		!input.trusted ||
		input.boundary ||
		Date.now() - input.statusAt > 30000 ||
		(input.status.scoreCap !== null &&
			input.status.scores.some((s) => s.score >= input.status.scoreCap!))
	)
		return persist('观测尚未稳定或正在换局，暂停调队。');
	const match = await currentMatch(env, server.id);
	if (
		!match ||
		match.map !== input.status.map ||
		now - Math.max(new Date(match.started_at).getTime(), input.startupAt) <
			config.graceSeconds * 1000
	)
		return persist('等待开局或重连保护时间结束。');
	if (config.waitMatchId === Number(match.id))
		return persist('原生人数差限制将在下一局解除；当前等待换局。');
	if (await conflicts(env, server.id))
		return persist('检测到禁止自行换边或强弱平衡已启用，暂停配额调队。');
	const teams = quotaTeams(input.status.scores);
	if (!teams) return persist('无法唯一识别蓝、红、绿阵营，暂停调队。');
	if (new Set(input.players.map((p) => p.steamId)).size !== input.players.length)
		return persist('玩家快照含重复身份，暂停调队。');
	// Verify that another administrator has not restored the native three-way join lock.
	if (now - (runtime.lastCheck ?? 0) >= 30000) {
		runtime.lastCheck = now;
		runtime.nativeReady = false;
		try {
			const doc = await readConfig(client);
			runtime.nativeReady =
				getScalar(parseIni(doc.text), section, joinKey)?.toLowerCase() === 'false';
		} catch {
			return persist('无法读取官方游戏配置，暂停调队；稍后重新检查。');
		}
		if (!runtime.nativeReady) return persist('原生人数差限制尚未解除，请重新保存启用设置。');
	}
	if (!runtime.nativeReady) return persist(runtime.reason || '等待官方游戏配置核对。');
	for (const event of runtime.events) {
		if (
			['sending', 'unknown'].includes(event.state) &&
			input.players.find((p) => p.steamId === event.steamId)?.faction === event.to
		) {
			event.state = 'confirmed';
			event.reason = '已在新玩家快照中确认目标阵营。';
		}
	}
	if (
		runtime.events.some(
			(e) => ['sending', 'unknown'].includes(e.state) && now - Date.parse(e.at) < 120000
		)
	)
		return persist('等待上一条调队在新快照中确认，暂不占用更多名额。');
	const blocked = new Set(
		runtime.events.filter((e) => now - Date.parse(e.at) < 120000).map((e) => e.steamId)
	);
	const plan = planQuota(input.players, teams, config.limits, blocked);
	runtime.counts = plan.counts;
	runtime.targets = plan.targets;
	// One move then obtain a fresh roster. Never fill several places based on an unconfirmed move.
	const move = plan.moves[0];
	if (!move) return persist(plan.reason);
	const current = await quotaConfig(env, server.id);
	if (!current.enabled || current.revision !== config.revision || !isOwner()) return;
	const event: QuotaEvent = {
		...move,
		at: input.now.toISOString(),
		state: 'sending',
		reason: '已提交调队，等待新快照确认。',
		revision: config.revision
	};
	runtime.events = [event, ...runtime.events].slice(0, 50);
	await withOwnedTransaction(env, async (tx) => {
		await tx.insert(factionMovePermits).values({
			serverId: server.id,
			steamId: move.steamId,
			faction: move.to,
			expiresAt: new Date(now + 120000)
		});
		await tx
			.insert(siteSettings)
			.values({ key: runtimeKey(server.id), value: runtime, updatedAt: input.now })
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value: runtime, updatedAt: input.now }
			});
	});
	try {
		if (!isOwner()) return;
		if (Date.now() - input.statusAt > 30000) {
			event.state = 'cancelled';
			event.reason = '观测已过期，未执行调队。';
			return persist(event.reason);
		}
		const latest = await quotaConfig(env, server.id);
		if (!latest.enabled || latest.revision !== config.revision) {
			event.state = 'cancelled';
			event.reason = '设置已改变，未执行调队。';
			return persist(event.reason);
		}
		await ACTIONS.changeTeam.run(client, { steamId: move.steamId, faction: move.to });
		await persist('调队已发送；将在下一次玩家快照核对人数。');
		await writeAudit(env, null, {
			actorName: '50V50 阵营配额',
			server,
			orgId: server.orgId,
			category: 'trigger',
			action: 'faction_quota.move',
			target: move.steamId,
			outcome: 'ok',
			detail: move
		});
	} catch {
		event.state = 'unknown';
		event.reason = '请求失败或结果不确定；两分钟内不重复操作该玩家，请核对阵营。';
		await persist(event.reason);
		await writeAudit(env, null, {
			actorName: '50V50 阵营配额',
			server,
			orgId: server.orgId,
			category: 'trigger',
			action: 'faction_quota.move',
			target: move.steamId,
			outcome: 'error',
			message: event.reason,
			detail: move
		});
	}
}
