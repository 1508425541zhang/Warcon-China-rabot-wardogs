import { numericLimitRules } from '$lib/server/db/schema';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { apiJson, param, readJson, route, ApiError } from '$lib/server/http';
import { validNumericLimits } from '$lib/numeric-limit-policy';
import { writeAudit } from '$lib/server/audit';
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, access, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	if (!access.caps.has('players.moderate')) throw new ApiError(403, '需要玩家管理权限。');
	const body = await readJson(event.request);
	if (!validNumericLimits(body))
		throw new ApiError(
			400,
			'窗口为60～900秒，最低击杀数为1～1000；上限需填写正数，不启用的项目设为null。'
		);
	const { enabled, kpm, kd, cash, windowSeconds, minKills } = body;
	const values = {
		serverId: server.id,
		config: { enabled, kpm, kd, cash, windowSeconds, minKills },
		updatedAt: new Date()
	};
	await env.db
		.insert(numericLimitRules)
		.values(values)
		.onConflictDoUpdate({ target: numericLimitRules.serverId, set: values });
	await writeAudit(env, event.request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'numeric_limit.settings',
		outcome: 'ok',
		detail: values.config
	});
	return apiJson({ ok: true });
});
