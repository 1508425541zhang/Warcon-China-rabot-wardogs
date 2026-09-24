// The server's kill feed setup: whether a token exists and when the last batch came (anyone who
// can see the server), the token itself and minting or removing it (org owners, like the rest of
// the server's credentials).
import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { getServer, orgRoleFor, requireServerCap, requireServerManager } from '$lib/server/access';
import { feedSetup, mintFeedToken, removeFeedToken } from '$lib/server/feed';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'server.view'
	);
	const owner = !user.apiKey && (await orgRoleFor(env, user, server.orgId)) === 'owner';
	return apiJson({ ok: true, ...(await feedSetup(env, server, owner)) });
});

export const POST = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerManager(env, event.locals, param(event, 'id'));
	const token = await mintFeedToken(env, event.request, user, server);
	const fresh = (await getServer(env, server.id)) ?? server;
	return apiJson({ ok: true, ...(await feedSetup(env, fresh, true)), token });
});

export const DELETE = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerManager(env, event.locals, param(event, 'id'));
	await removeFeedToken(env, event.request, user, server);
	return apiJson({ ok: true, configured: false });
});
