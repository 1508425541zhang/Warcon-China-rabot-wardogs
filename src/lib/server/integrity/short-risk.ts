import { readFile } from 'node:fs/promises';
import { env as privateEnv } from '$env/dynamic/private';
import { eq, like, desc, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { servers, serverLive, siteSettings } from '../db/schema';
import { isOwner, withOwnedTransaction } from '../leadership';
import { gateway } from '../gateway';
import { writeAudit } from '../audit';
import { getIntegrityRules } from './rules';
import { vipFor } from '../qq/vip';
import type { Player } from '$lib/types';
import {
	shortRiskDecision,
	freshShortSnapshot,
	shortWindowConsensus,
	type ShortWindow
} from '$lib/short-risk-policy';
import { shortModelEnabled } from '$lib/integrity-engines';

export type ShortPlayer = {
	server_id: string;
	player_id: string;
	name?: string;
	ShortRisk: number | null;
	anomaly_score?: number;
	percentile?: number;
	scope?: string[];
	status: string;
	recent_windows?: ShortWindow[];
};
type Snapshot = {
	evaluated_at: string;
	model_sha256: string;
	algorithm: string;
	players: ShortPlayer[];
};
type RecordValue = {
	serverId: string;
	steamId: string;
	name: string;
	score: number;
	percentile: number;
	scope: string[];
	state: string;
	reason?: string;
	attemptedAt?: string;
	updatedAt: string;
	windowScores?: ShortWindow[];
	weightedScore?: number;
};

async function snapshot(): Promise<Snapshot | null> {
	try {
		const raw = await readFile('/var/lib/warcon-short-risk/status.json', 'utf8');
		if (raw.length > 2_000_000) return null;
		const x = JSON.parse(raw) as Snapshot;
		if (!freshShortSnapshot(x)) return null;
		return x;
	} catch {
		return null;
	}
}
export async function loadShortRisk(env: Env, serverId: string) {
	const x = await snapshot();

	return {
		available: !!x,
		evaluatedAt: x?.evaluated_at ?? null,
		autoKick: privateEnv.WARCON_SHORT_RISK_ENABLED === '1',
		warningPercentile: 99.6,
		kickPercentile: 99.9,
		players: (x?.players ?? [])
			.filter((p) => p.server_id === serverId && typeof p.anomaly_score === 'number')
			.map((p) => ({ ...p, level: shortRiskDecision(p.anomaly_score!) }))
			.sort((a, b) => b.anomaly_score! - a.anomaly_score!),
		history: [] as RecordValue[]
	};
}
let timer: ReturnType<typeof setInterval> | undefined;
let busy = false;
export function startShortRisk(env: Env) {
	if (timer || privateEnv.WARCON_SHORT_RISK_ENABLED !== '1') return;
	timer = setInterval(() => {
		if (!busy && isOwner()) {
			busy = true;
			void tick(env)
				.catch((e) => console.error('[short-risk]', e instanceof Error ? e.name : 'error'))
				.finally(() => {
					busy = false;
				});
		}
	}, 10000);
}
export function stopShortRisk() {
	if (timer) clearInterval(timer);
	timer = undefined;
}
async function tick(env: Env) {
	const active = await env.db.execute(
		sql`SELECT 1 FROM integrity_rules WHERE assessment_mode IN ('short_only','model_only') LIMIT 1`
	);
	if (!active.length) return;
	const x = await snapshot();
	if (!x) return;
	for (const p of x.players) {
		if (
			!/^\d{17}$/.test(p.player_id) ||
			typeof p.anomaly_score !== 'number' ||
			shortRiskDecision(p.anomaly_score) === 'normal' ||
			!Array.isArray(p.scope) ||
			p.scope.length !== 4
		)
			continue;
		const [server] = await env.db.select().from(servers).where(eq(servers.id, p.server_id));
		if (!server) continue;
		const rules = await getIntegrityRules(env, server.orgId);
		if (!shortModelEnabled(rules.assessmentMode)) continue;
		const key = `shortRisk:${server.id}:${p.player_id}:${p.scope[1]}:${p.scope[2]}`;
		const now = new Date();
		const record: RecordValue = {
			serverId: server.id,
			steamId: p.player_id,
			name: p.name ?? p.player_id,
			score: p.anomaly_score,
			percentile: p.percentile ?? 0,
			scope: p.scope,
			state: 'warning',
			updatedAt: now.toISOString()
		};
		const claimed = await withOwnedTransaction(env, async (tx) => {
			await tx.execute(
				sql`SELECT pg_advisory_xact_lock(hashtextextended(${'shortRisk:' + server.orgId},0))`
			);
			const active = await tx.execute(
				sql`SELECT 1 FROM integrity_rules WHERE org_id=${server.orgId} AND assessment_mode IN ('short_only','model_only')`
			);
			if (!active.length) return false;
			const [prior] = await tx.select().from(siteSettings).where(eq(siteSettings.key, key));
			const saved = prior?.value as RecordValue | undefined;
			if (saved?.attemptedAt) return false; // crash/unknown outcomes are never blindly re-sent.
			let reason = '';
			const [live] = await tx.select().from(serverLive).where(eq(serverLive.serverId, server.id));
			const players = (live?.players ?? []) as Player[];
			const healthy =
				live?.ok &&
				[live.feedAt, live.statusAt, live.playersAt].every(
					(t) => t && now.getTime() - t.getTime() >= 0 && now.getTime() - t.getTime() < 30000
				);
			const [match] = await tx.execute(
				sql`SELECT id FROM matches WHERE server_id=${server.id} AND ended_at IS NULL ORDER BY id DESC LIMIT 1`
			);
			if (!healthy || !players.some((a) => a.steamId === p.player_id))
				reason = '玩家离线或数据不健康';
			else if (String(match?.id) !== p.scope![2]) reason = '对局已变化';
			else if (players.length < rules.config.minimumOnlineForAutoAction) reason = '人数不足';
			else if (rules.enforcement.autoSuspendedAt) reason = '自动处罚已暂停';
			else if ((await vipFor({ db: tx }, server.id, p.player_id))?.whitelist) reason = 'VIP 白名单';
			const [count] = await tx.execute(
				sql`SELECT count(*) AS n FROM site_settings WHERE key LIKE ${'shortRisk:%'} AND value->>'serverId' IN (SELECT id FROM servers WHERE org_id=${server.orgId}) AND (value->>'attemptedAt')::timestamptz>now()-interval '1 hour'`
			);
			if (
				Number(count.n) >=
				Math.min(
					rules.enforcement.autoActionMaxPerHour,
					Math.max(
						1,
						Math.floor((players.length * rules.enforcement.autoActionMaxPercentOnline) / 100)
					)
				)
			)
				reason = '达到自动处罚频率上限';
			const weightedScore = shortWindowConsensus(p.recent_windows, x.evaluated_at);
			const kick = weightedScore !== null && !reason;
			record.windowScores = p.recent_windows;
			if (weightedScore !== null) record.weightedScore = weightedScore;
			record.reason =
				reason ||
				(kick
					? '连续五窗均超过 P99.9，按 1:2:3:4:5 加权确认踢出'
					: 'P99.6 警惕；等待连续五窗 P99.9 加权确认');
			if (kick) {
				record.state = 'pending';
				record.attemptedAt = now.toISOString();
			}
			await tx
				.insert(siteSettings)
				.values({ key, value: record, updatedAt: now })
				.onConflictDoUpdate({ target: siteSettings.key, set: { value: record, updatedAt: now } });
			return kick;
		});
		if (!claimed) continue;
		try {
			if (!isOwner()) throw new Error('Worker ownership lost');
			if (!shortModelEnabled((await getIntegrityRules(env, server.orgId)).assessmentMode))
				throw new Error('Short model disabled');
			await gateway().run(env, server, 'kick', {
				steamId: p.player_id,
				reason: '短窗连续五窗均超过 P99.9，加权确认异常，请联系管理员复核。'
			});
			record.state = 'delivered';
			record.reason = '连续五窗 P99.9 加权确认；游戏服务器已接受踢出请求';
		} catch {
			record.state = 'unknown';
			record.reason = '请求未确认，保留记录且不自动重发';
		}
		await env.db
			.update(siteSettings)
			.set({ value: record, updatedAt: new Date() })
			.where(eq(siteSettings.key, key));
		await writeAudit(env, null, {
			orgId: server.orgId,
			server,
			actorName: '无监督短窗 P99.9',
			category: 'trigger',
			action: 'shortRisk.kick',
			target: p.player_id,
			outcome: record.state === 'delivered' ? 'ok' : 'error',
			message: record.reason,
			detail: record
		});
	}
}
