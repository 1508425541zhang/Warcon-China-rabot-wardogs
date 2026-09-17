import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { getOrg, requireServerCap } from '$lib/server/access';
import { addServerEntry } from '$lib/server/lists';

/** Reserves a slot on this server alone, through the server's own list (note and expiry as the org list takes them). */
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'slots.manage'
	);
	const org = await getOrg(env, server.orgId);
	if (!org) throw new ApiError(404, 'Organisation not found.');
	const result = await addServerEntry(
		env,
		event.request,
		user,
		server,
		org,
		await readJson(event.request)
	);
	return apiJson({ ok: true, ...result }, 201);
});
