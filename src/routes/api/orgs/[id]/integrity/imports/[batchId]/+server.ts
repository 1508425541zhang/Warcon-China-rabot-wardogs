import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { reviewIntegrityImport } from '$lib/server/integrity/imports';

export const POST = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const body = await readJson<{ decision?: unknown; confirmation?: unknown }>(event.request);
	if (body.decision !== 'APPROVED' && body.decision !== 'REJECTED')
		throw new ApiError(400, 'decision 必须是 APPROVED 或 REJECTED。');
	if (body.decision === 'APPROVED' && body.confirmation !== 'APPROVE_EXTERNAL_INTEGRITY_DATA')
		throw new ApiError(400, '批准外部数据需要明确确认。');
	return apiJson({
		ok: true,
		batch: await reviewIntegrityImport(
			env,
			event.request,
			user,
			org.id,
			param(event, 'batchId'),
			body.decision
		)
	});
});
