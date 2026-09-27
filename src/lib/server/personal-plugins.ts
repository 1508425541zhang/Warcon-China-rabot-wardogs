import { and, desc, eq, sql } from 'drizzle-orm';
import { z } from 'zod';
import type { Env } from './env';
import { personalPlugins, serverLive } from './db/schema';
import { ApiError } from './http';
import { requireServerCap, requireUser } from './access';
import { pluginManifestSchema, type PersonalPlugin, type PluginSnapshot } from '$lib/plugins/sdk';
import { knownPluginRenderer } from '$lib/plugins/catalog';
import type { Player, Status } from '$lib/types';

export function pluginUser(locals: App.Locals) {
	const user = requireUser(locals);
	if (locals.apiKey || user.apiKey)
		throw new ApiError(
			403,
			'个人插件设置需要账号会话；组织 API 密钥请使用服务器快照接口。',
			'api_key_forbidden'
		);
	return user;
}
const installSchema = z
	.object({
		manifest: pluginManifestSchema,
		serverId: z.string().min(1).max(64).nullable().default(null),
		enabled: z.boolean().default(true)
	})
	.strict();
const view = (row: typeof personalPlugins.$inferSelect): PersonalPlugin => ({
	manifest: pluginManifestSchema.parse(row.manifest),
	serverId: row.serverId,
	enabled: row.enabled,
	updatedAt: row.updatedAt.toISOString()
});

export async function readPluginJson(request: Request): Promise<unknown> {
	if (!request.headers.get('content-type')?.includes('application/json'))
		throw new ApiError(415, '请使用 application/json。');
	const reader = request.body?.getReader();
	if (!reader) throw new ApiError(400, '缺少 JSON 内容。');
	const decoder = new TextDecoder();
	let bytes = 0;
	let text = '';
	try {
		for (;;) {
			const part = await reader.read();
			if (part.done) break;
			bytes += part.value.byteLength;
			if (bytes > 65_536) {
				await reader.cancel();
				throw new ApiError(413, '插件配置不得超过 64 KB。');
			}
			text += decoder.decode(part.value, { stream: true });
		}
		text += decoder.decode();
		try {
			return JSON.parse(text);
		} catch {
			throw new ApiError(400, 'JSON 格式不正确。', 'invalid_json');
		}
	} finally {
		reader.releaseLock();
	}
}
export async function listPersonalPlugins(env: Env, locals: App.Locals) {
	const user = pluginUser(locals);
	return (
		await env.db
			.select()
			.from(personalPlugins)
			.where(eq(personalPlugins.userId, user.id))
			.orderBy(desc(personalPlugins.updatedAt))
	).map(view);
}
export async function getPersonalPlugin(env: Env, locals: App.Locals, id: string) {
	const user = pluginUser(locals);
	const [row] = await env.db
		.select()
		.from(personalPlugins)
		.where(and(eq(personalPlugins.userId, user.id), eq(personalPlugins.pluginId, id)));
	if (!row) throw new ApiError(404, '找不到这个个人插件。', 'not_found');
	return view(row);
}
export async function savePersonalPlugin(env: Env, locals: App.Locals, raw: unknown, id?: string) {
	const user = pluginUser(locals);
	const result = installSchema.safeParse(raw);
	if (!result.success)
		throw new ApiError(
			400,
			result.error.issues.map((i) => `${i.path.join('.')}: ${i.message}`).join('；'),
			'invalid_plugin'
		);
	const input = result.data;
	if (!knownPluginRenderer(input.manifest.renderer))
		throw new ApiError(
			400,
			'此代码插件尚未随服务器构建安装，请先注册组件再部署。',
			'unknown_renderer'
		);
	if (id && id !== input.manifest.id)
		throw new ApiError(400, '编辑时不能修改插件标识；请用新标识添加副本。');
	if (input.serverId) await requireServerCap(env, locals, input.serverId, 'server.view');
	return env.db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${'personal-plugins:' + user.id}, 0))`
		);
		const rows = await tx
			.select({ id: personalPlugins.pluginId })
			.from(personalPlugins)
			.where(eq(personalPlugins.userId, user.id));
		const exists = rows.some((r) => r.id === input.manifest.id);
		if (id && !exists) throw new ApiError(404, '找不到这个个人插件。', 'not_found');
		if (!id && exists)
			throw new ApiError(409, '同名标识已存在，请编辑已有插件或更换标识。', 'plugin_exists');
		if (!exists && rows.length >= 20)
			throw new ApiError(409, '每个账号最多保存 20 个个人插件。', 'plugin_limit');
		const values = {
			manifest: input.manifest,
			serverId: input.serverId,
			enabled: input.enabled,
			updatedAt: new Date()
		};
		const [saved] = id
			? await tx
					.update(personalPlugins)
					.set(values)
					.where(and(eq(personalPlugins.userId, user.id), eq(personalPlugins.pluginId, id)))
					.returning()
			: await tx
					.insert(personalPlugins)
					.values({ ...values, userId: user.id, pluginId: input.manifest.id })
					.returning();
		return view(saved);
	});
}
export async function deletePersonalPlugin(env: Env, locals: App.Locals, id: string) {
	const user = pluginUser(locals);
	const removed = await env.db
		.delete(personalPlugins)
		.where(and(eq(personalPlugins.userId, user.id), eq(personalPlugins.pluginId, id)))
		.returning({ id: personalPlugins.pluginId });
	if (!removed.length) throw new ApiError(404, '找不到这个个人插件。', 'not_found');
}
const finite = (value: unknown): number | null =>
	typeof value === 'number' && Number.isFinite(value) ? value : null;
export async function loadPluginSnapshot(
	env: Env,
	locals: App.Locals,
	serverId: string
): Promise<PluginSnapshot> {
	const { server } = await requireServerCap(env, locals, serverId, 'server.view');
	const [live] = await env.db
		.select({
			status: serverLive.status,
			players: serverLive.players,
			statusAt: serverLive.statusAt,
			playersAt: serverLive.playersAt
		})
		.from(serverLive)
		.where(eq(serverLive.serverId, serverId));
	const status = live?.status as Status | null;
	const raw = Array.isArray(live?.players) ? (live.players as Player[]) : [];
	const players = raw
		.slice(0, 256)
		.filter((p) => p && typeof p === 'object')
		.map((p) => ({
			name: String(p.name ?? ''),
			steamId: String(p.steamId ?? ''),
			faction: typeof p.faction === 'string' ? p.faction : null,
			kills: finite(p.kills),
			deaths: finite(p.deaths),
			cash: finite(p.cash),
			ping: finite(p.ping)
		}));
	const hasPlayers = !!live?.playersAt && Array.isArray(live.players);
	const sum = (key: 'kills' | 'deaths' | 'cash') =>
		hasPlayers && raw.length <= 256 && players.every((p) => p[key] !== null)
			? players.reduce((s, p) => s + (p[key] ?? 0), 0)
			: null;
	const pings = players.flatMap((p) => (p.ping === null ? [] : [p.ping]));
	return {
		apiVersion: 1,
		server: {
			id: server.id,
			name: server.name,
			map: typeof status?.map === 'string' ? status.map : null
		},
		statusAt: live?.statusAt?.toISOString() ?? null,
		playersAt: live?.playersAt?.toISOString() ?? null,
		stale: !live?.playersAt || Date.now() - live.playersAt.getTime() > 90_000,
		metrics: {
			online: hasPlayers ? raw.length : null,
			kills: sum('kills'),
			deaths: sum('deaths'),
			cash: sum('cash'),
			averagePing:
				hasPlayers && raw.length <= 256 && pings.length
					? Math.round(pings.reduce((a, b) => a + b, 0) / pings.length)
					: null
		},
		players,
		playersTruncated: raw.length > 256
	};
}
