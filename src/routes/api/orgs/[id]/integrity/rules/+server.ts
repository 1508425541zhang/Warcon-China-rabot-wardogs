import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { getIntegrityRules, saveIntegrityRules } from '$lib/server/integrity/rules';

export const GET = route(async (event) => {
	const env = getEnv();
	const { org } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	return apiJson({ ok: true, rules: await getIntegrityRules(env, org.id) });
});

export const PUT = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const body = await readJson<{ values?: Record<string, unknown> }>(event.request);
	return apiJson({
		ok: true,
		rules: await saveIntegrityRules(env, event.request, user, org.id, body.values ?? {})
	});
});
