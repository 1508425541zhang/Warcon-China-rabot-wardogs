import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { saveAssessmentMode } from '$lib/server/integrity/rules';
import type { AssessmentMode } from '$lib/server/integrity/statistics';

export const PUT = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const body = await readJson<{ mode: AssessmentMode; confirmation?: string }>(event.request);
	return apiJson({
		ok: true,
		rules: await saveAssessmentMode(
			env,
			event.request,
			user,
			org.id,
			body.mode,
			body.confirmation ?? ''
		)
	});
});
