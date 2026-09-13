import { getEnv } from '$lib/server/env';
import { apiJson, param, readJson, route, str } from '$lib/server/http';
import { requireOrgRole } from '$lib/server/access';
import { assertRate } from '$lib/server/ratelimit';
import { sendTestCard } from '$lib/server/webhooks';

/** {serverId}: posts a throwaway status card for that server, removed a minute later. */
export const POST = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	assertRate(`webhook-test:${user.id}`, 10, 60_000);
	const body = await readJson(event.request);
	const result = await sendTestCard(
		env,
		event.request,
		user,
		org,
		param(event, 'webhookId'),
		str(body.serverId, 64)
	);
	return apiJson({ ok: result.ok, result }, result.ok ? 200 : 502);
});
