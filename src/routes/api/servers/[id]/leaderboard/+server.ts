// One page of the leaderboard: this server, or every server of its organisation the caller can
// see. Read at page load from kills, player_sessions and matches; nothing is precomputed.
// ?scope=server|org&range=7d|30d|90d|all&sort=<metric>&dir=asc|desc&page=1&minMinutes=60
import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { accessibleServers, requireServerCap } from '$lib/server/access';
import { loadBoard } from '$lib/server/leaderboards';
import { parseBoardQuery } from '$lib/leaderboard';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'server.view'
	);
	const q = parseBoardQuery(event.url.searchParams);
	const ids =
		q.scope === 'org'
			? (await accessibleServers(env, user, server.orgId)).map((s) => s.id)
			: [server.id];
	return apiJson({ ok: true, ...(await loadBoard(env, ids, q)) });
});
