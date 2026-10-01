import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { apiJson, param, readJson, route, ApiError } from '$lib/server/http';
import { factionQuotaView, saveFactionQuota } from '$lib/server/faction-quota';
import { writeAudit } from '$lib/server/audit';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	return apiJson(await factionQuotaView(env, server.id));
});
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, access, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	if (!access.caps.has('players.moderate') || !access.caps.has('config.apply'))
		throw new ApiError(403, '需要自动化、玩家调队及游戏配置权限。');
	const config = await saveFactionQuota(env, server, user.id, await readJson(event.request));
	await writeAudit(env, event.request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'faction_quota.settings',
		outcome: 'ok',
		detail: config
	});
	return apiJson({ config });
});
