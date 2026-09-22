// The server's match history: newest first, fifty a page, by `page`. The list reads the matches
// table alone; a match's lines are on its own route.
import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { matchListView, MATCHES_PAGE } from '$lib/server/matches';
import { parsePage } from '$lib/matches';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, event.locals, param(event, 'id'), 'server.view');
	const page = parsePage(event.url.searchParams.get('page'));
	return apiJson({ ok: true, ...(await matchListView(env, server.id, page, MATCHES_PAGE)) });
});
