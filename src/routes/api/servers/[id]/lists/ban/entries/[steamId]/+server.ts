import type { RequestEvent } from '@sveltejs/kit';
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { getOrg, requireServerCap } from '$lib/server/access';
import { removeServerEntry, updateEntry } from '$lib/server/lists';

async function target(event: RequestEvent) {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'bans.manage'
	);
	const org = await getOrg(env, server.orgId);
	if (!org) throw new ApiError(404, 'Organisation not found.');
	return { env, server, user, org };
}

/** Changes the reason or the expiry of a ban on the server's own list. */
export const PATCH = route(async (event) => {
	const { env, server, user, org } = await target(event);
	const result = await updateEntry(
		env,
		event.request,
		user,
		org,
		server,
		'ban',
		param(event, 'steamId'),
		await readJson(event.request)
	);
	return apiJson({ ok: true, ...result });
});

/** Lifts a ban the server's own list holds. */
export const DELETE = route(async (event) => {
	const { env, server, user, org } = await target(event);
	const result = await removeServerEntry(
		env,
		event.request,
		user,
		server,
		org,
		'ban',
		param(event, 'steamId')
	);
	return apiJson({ ok: true, ...result });
});
