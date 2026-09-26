import { and, eq, inArray } from 'drizzle-orm';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { apiJson, param, readJson, route, ApiError } from '$lib/server/http';
import { skillBalanceRules, skillBalanceRuns } from '$lib/server/db/schema';
import { writeAudit } from '$lib/server/audit';

export const POST = route(async (event) => {
	const env = getEnv();
	const { server, access, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	if (!access.caps.has('players.moderate')) throw new ApiError(403, '需要玩家调队权限。');
	const body = await readJson(event.request);
	if (
		typeof body.enabled !== 'boolean' ||
		!Number.isInteger(body.graceSeconds) ||
		Number(body.graceSeconds) < 180 ||
		Number(body.graceSeconds) > 1800 ||
		!Number.isInteger(body.leadPoints) ||
		Number(body.leadPoints) < 40 ||
		Number(body.leadPoints) > 10000
	)
		throw new ApiError(400, '开局保护应为180～1800秒；领先分差应为40～10000分。');
	const values = {
		serverId: server.id,
		enabled: body.enabled,
		graceSeconds: Number(body.graceSeconds),
		leadPoints: Number(body.leadPoints),
		updatedAt: new Date()
	};
	await env.db.transaction(async (tx) => {
		await tx
			.insert(skillBalanceRules)
			.values(values)
			.onConflictDoUpdate({ target: skillBalanceRules.serverId, set: values });
		await tx
			.update(skillBalanceRuns)
			.set({
				state: 'cancelled',
				reason: values.enabled
					? '配置已变更，本局计划停止，等待下一局'
					: '管理员已关闭平衡；剩余玩家停止调队',
				updatedAt: new Date()
			})
			.where(
				and(
					eq(skillBalanceRuns.serverId, server.id),
					inArray(skillBalanceRuns.state, ['waiting_safe', 'waiting_death'])
				)
			);
	});
	await writeAudit(env, event.request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'skill_balance.settings',
		outcome: 'ok',
		detail: values
	});
	return apiJson({ ok: true });
});
