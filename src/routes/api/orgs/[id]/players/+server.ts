// Everyone the organisation has seen on the servers the caller can open, paged.
import { getEnv } from '$lib/server/env';
import { apiJson, int, param, route } from '$lib/server/http';
import { accessibleServers, requireListsRole } from '$lib/server/access';
import { seenFilters, seenPlayers } from '$lib/server/seen';

export const GET = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireListsRole(env, event.locals, param(event, 'id'), 'any');
	const serverIds = (await accessibleServers(env, user, org.id)).map((s) => s.id);
	const p = event.url.searchParams;
	const { players, total } = await seenPlayers(env, {
		orgId: org.id,
		serverIds,
		filters: seenFilters(p),
		limit: int(p.get('limit'), 100, 1, 200),
		offset: int(p.get('offset'), 0, 0, 1_000_000),
		steam: true
	});
	return apiJson({ ok: true, players, total });
});
