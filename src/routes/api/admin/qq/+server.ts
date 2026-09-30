import { getEnv } from '$lib/server/env';
import { requireOwner } from '$lib/server/access';
import { apiJson, readJson, route } from '$lib/server/http';
import { writeAudit } from '$lib/server/audit';
import { qqSettingsView, saveQqSettings, testQqConnection } from '$lib/server/qq/settings';

export const GET = route(async ({ locals }) => {
	requireOwner(locals);
	return apiJson({ ok: true, config: await qqSettingsView(getEnv()) });
});
export const PUT = route(async ({ locals, request }) => {
	const user = requireOwner(locals);
	const env = getEnv();
	const config = await saveQqSettings(env, await readJson(request), user.id);
	await writeAudit(env, request, {
		actor: user,
		category: 'system',
		action: 'qq.settings.update',
		target: 'qqCommunity',
		outcome: 'ok',
		detail: {
			revision: config.revision,
			provider: config.provider,
			enabled: config.enabled,
			servers: config.policies.map((p) => p.serverId)
		}
	});
	return apiJson({ ok: true, config });
});
export const POST = route(async ({ locals, request }) => {
	const user = requireOwner(locals);
	const env = getEnv();
	const result = await testQqConnection(env);
	await writeAudit(env, request, {
		actor: user,
		category: 'system',
		action: 'qq.connection.test',
		target: 'qqCommunity',
		outcome: 'ok'
	});
	return apiJson(result);
});
