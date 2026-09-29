import { requireOwner } from '$lib/server/access';
import { getEnv } from '$lib/server/env';
import { apiJson, readJson, route } from '$lib/server/http';
import { writeAudit } from '$lib/server/audit';
import { vipSettings, saveVips } from '$lib/server/qq/vip';
export const GET = route(async ({ locals }) => {
	requireOwner(locals);
	return apiJson({ ok: true, vips: await vipSettings(getEnv()) });
});
export const PUT = route(async ({ locals, request }) => {
	const actor = requireOwner(locals),
		env = getEnv();
	const vips = await saveVips(env, await readJson(request), actor.id);
	await writeAudit(env, request, {
		actor,
		category: 'system',
		action: 'qq.vip.update',
		target: 'qqVip',
		outcome: 'ok',
		detail: vips
	});
	return apiJson({ ok: true, vips });
});
