import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { getOrg, requireServerCap } from '$lib/server/access';
import { addServerEntry } from '$lib/server/lists';

/**
 * Bans a player on this server alone, through the server's own list: a reason and an expiry as
 * the org list takes them, and a player who is not connected is banned the moment they are seen.
 */
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'bans.manage'
	);
	const org = await getOrg(env, server.orgId);
	if (!org) throw new ApiError(404, 'Organisation not found.');
	const result = await addServerEntry(
		env,
		event.request,
		user,
		server,
		org,
		'ban',
		await readJson(event.request)
	);
	return apiJson({ ok: true, ...result }, 201);
});
