import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { labelIntegrityCase } from '$lib/server/integrity/labels';

export const POST = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const body = await readJson<{ label?: unknown; reason?: unknown }>(event.request);
	return apiJson({
		ok: true,
		label: await labelIntegrityCase(
			env,
			event.request,
			user,
			org.id,
			param(event, 'caseId'),
			body.label,
			body.reason
		)
	});
});
