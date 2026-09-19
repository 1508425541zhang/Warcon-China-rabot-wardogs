// Everyone who has played on this server, from its own sessions, paged: the Players tab's past
// players. Who has been on a server is View (the leaderboards and analytics say as much); nothing
// of the org's other servers is read, whatever the query asks for.
import { getEnv } from '$lib/server/env';
import { apiJson, int, param, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { assertRate } from '$lib/server/ratelimit';
import { seenFilters, seenPlayers } from '$lib/server/seen';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'server.view'
	);
	// One aggregate over the server's sessions per call: enough for typing a search, not a scrape.
	assertRate(`seen:${user.id}`, 60, 60_000);
	const p = event.url.searchParams;
	const { players, total } = await seenPlayers(env, {
		orgId: server.orgId,
		serverIds: [server.id],
		filters: { ...seenFilters(p), serverId: '' },
		limit: int(p.get('limit'), 50, 1, 100),
		offset: int(p.get('offset'), 0, 0, 1_000_000)
	});
	return apiJson({ ok: true, players, total });
});
