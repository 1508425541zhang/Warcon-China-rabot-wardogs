import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, route } from '$lib/server/http';
import { getOrg, requireServerCap } from '$lib/server/access';
import { removeServerEntry } from '$lib/server/lists';

/** Withdraws a slot the server's own list holds. */
export const DELETE = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'slots.manage'
	);
	const org = await getOrg(env, server.orgId);
	if (!org) throw new ApiError(404, 'Organisation not found.');
	const result = await removeServerEntry(
		env,
		event.request,
		user,
		server,
		org,
		param(event, 'steamId')
	);
	return apiJson({ ok: true, ...result });
});
