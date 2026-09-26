import { and, eq, inArray } from 'drizzle-orm';
import { getEnv } from '$lib/server/env';
import { apiJson, param, readJson, route, ApiError } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { factionLockRules, factionLockEvents } from '$lib/server/db/schema';
import { writeAudit } from '$lib/server/audit';
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, access, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	if (!access.caps.has('players.moderate') || !access.caps.has('chat.send'))
		throw new ApiError(403, '需要调队、踢出及私信权限。');
	const body = await readJson(event.request);
	if (
		typeof body.enabled !== 'boolean' ||
		typeof body.graceSeconds !== 'number' ||
		!Number.isInteger(body.graceSeconds) ||
		body.graceSeconds < 30 ||
		body.graceSeconds > 600
	)
		throw new ApiError(400, '保护时间应为 30～600 秒。');
	if (!body.capacities || typeof body.capacities !== 'object' || Array.isArray(body.capacities))
		throw new ApiError(400, '阵营容量格式错误。');
	const capacities: Record<string, number> = {};
	for (const [name, value] of Object.entries(body.capacities)) {
		if (
			!name ||
			name.length > 100 ||
			typeof value !== 'number' ||
			!Number.isInteger(value) ||
			value < 1 ||
			value > 1000
		)
			throw new ApiError(400, '阵营容量应为 1～1000 的整数；未知请留空。');
		capacities[name] = value;
	}
	const values = {
		serverId: server.id,
		enabled: body.enabled,
		graceSeconds: body.graceSeconds,
		capacities,
		updatedAt: new Date()
	};
	await env.db
		.insert(factionLockRules)
		.values(values)
		.onConflictDoUpdate({ target: factionLockRules.serverId, set: values });
	if (!values.enabled)
		await env.db
			.update(factionLockEvents)
			.set({ state: 'skipped', reason: '管理员已关闭功能', updatedAt: new Date() })
			.where(
				and(
					eq(factionLockEvents.serverId, server.id),
					inArray(factionLockEvents.state, ['pending', 'warned'])
				)
			);
	await writeAudit(env, event.request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'faction_lock.settings',
		outcome: 'ok',
		detail: values
	});
	return apiJson({ ok: true });
});
