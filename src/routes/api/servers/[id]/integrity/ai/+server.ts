import { getEnv } from '$lib/server/env';
import { requireServerCap, orgRoleFor } from '$lib/server/access';
import { route, apiJson, readJson, ApiError } from '$lib/server/http';
import { aiBundle, aiCall, saveAiSettings } from '$lib/server/integrity/ai';
import { integrityAiSettings } from '$lib/server/db/schema';
import { writeAudit } from '$lib/server/audit';
import { eq } from 'drizzle-orm';
export const POST = route(async ({ locals, params, request }) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(env, locals, params.id!, 'integrity.view');
	const body = await readJson<{ operation?: string; caseId?: string; settings?: unknown }>(request);
	const owner = (await orgRoleFor(env, user, server.orgId)) === 'owner';
	if (body.operation === 'preview' || body.operation === 'review') {
		if (typeof body.caseId !== 'string' || body.caseId.length > 200)
			throw new ApiError(400, '请选择案件。');
		const bundle = await aiBundle(env, server.orgId, server.id, body.caseId);
		if (body.operation === 'preview') return apiJson({ bundle });
		const result = await aiCall(env, server.orgId, 'review', bundle);
		await writeAudit(env, request, {
			actor: user,
			server,
			orgId: server.orgId,
			category: 'server',
			action: 'integrity.ai.review',
			target: body.caseId,
			outcome: 'ok'
		});
		return apiJson(result);
	}
	if (!owner) throw new ApiError(403, '仅组织管理员可以配置或测试模型接口。');
	if (body.operation === 'save') {
		await saveAiSettings(env, server.orgId, body.settings);
		await writeAudit(env, request, {
			actor: user,
			server,
			orgId: server.orgId,
			category: 'server',
			action: 'integrity.ai.configure',
			outcome: 'ok'
		});
		return apiJson({ ok: true });
	}
	if (body.operation === 'delete') {
		await env.db.delete(integrityAiSettings).where(eq(integrityAiSettings.orgId, server.orgId));
		return apiJson({ ok: true });
	}
	if (body.operation === 'models' || body.operation === 'test')
		return apiJson(await aiCall(env, server.orgId, body.operation));
	throw new ApiError(400, '未知操作。');
});
