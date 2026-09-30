import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { modelConfigView, saveModelConfig, testModel } from '$lib/server/integrity/model-http';
import { modelRuns } from '$lib/server/integrity/model-runtime';
import { writeAudit } from '$lib/server/audit';

export const GET = route(async (event) => {
	const env = getEnv();
	const { org } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	return apiJson({
		ok: true,
		config: await modelConfigView(env, org.id),
		runs: await modelRuns(env, org.id)
	});
});
export const PUT = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const config = await saveModelConfig(env, org.id, user.id, await readJson(event.request));
	await writeAudit(env, event.request, {
		actor: user,
		orgId: org.id,
		category: 'org',
		action: 'integrity.model.configure',
		outcome: 'ok',
		message: 'A测模型 HTTP 配置已更新',
		detail: config
	});
	return apiJson({ ok: true, config });
});
export const POST = route(async (event) => {
	const env = getEnv();
	const { org } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	return apiJson({ ok: true, ...(await testModel(env, org.id)) });
});
