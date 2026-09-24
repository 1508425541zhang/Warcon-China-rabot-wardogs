// Purges the server's stats: its kills, matches and match rows, for good. Org owners only (like
// deleting the server), never an API key, and the body has to carry the server's name back.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { requireServerManager } from '$lib/server/access';
import { purgeServerStats } from '$lib/server/stats';

export const POST = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerManager(env, event.locals, param(event, 'id'));
	const body = await readJson<{ name?: unknown }>(event.request);
	if (typeof body.name !== 'string' || body.name.trim() !== server.name)
		throw new ApiError(400, "Type the server's name to confirm the purge.", 'name_mismatch');
	const counts = await purgeServerStats(env, event.request, user, server);
	return apiJson({ ok: true, counts });
});
