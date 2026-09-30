import { getEnv } from '$lib/server/env';
import { requireOrgRole, requireServerCap } from '$lib/server/access';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { saveHistoryPolicy } from '$lib/server/integrity/history-retention';
export const PUT = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'integrity.view'
	);
	const { user } = await requireOrgRole(env, event.locals, server.orgId, 'owner');
	const body = await readJson<{ policy: unknown; revision: string }>(event.request);
	if (typeof body.revision !== 'string')
		return apiJson({ error: { message: '缺少保留设置版本。' } }, 400);
	return apiJson({
		ok: true,
		...(await saveHistoryPolicy(env, user, server.id, body.policy, body.revision))
	});
});
