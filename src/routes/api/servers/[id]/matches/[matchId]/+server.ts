// One match of the server that has ended: every player's line, the score timeline and the
// awards. Its kills are the kills route with `match=<id>`. Another server's match is not found.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { loadMatch, parseMatchId } from '$lib/server/matches';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, event.locals, param(event, 'id'), 'server.view');
	const id = parseMatchId(param(event, 'matchId'));
	const view = id === null ? null : await loadMatch(env, server.id, id);
	if (!view) throw new ApiError(404, 'No such match here.', 'not_found');
	return apiJson({ ok: true, ...view });
});
