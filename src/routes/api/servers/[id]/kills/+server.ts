// The server's kill feed as stored: newest first, paged by `before` (an ISO timestamp), narrowed
// by the filter in $lib/kills (killer, victim, player, cause, kind, minM), plus whether a feed is
// set up and when its last batch arrived. `count=1` adds how many kills match over the whole
// history, which the Kills tab wants once per filter and the Overview panel never. Live updates
// come over the event stream (/api/live/events, event `kills`); this is a page's starting point
// and its history.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, int, param, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { countKills, feedSetup, recentKills } from '$lib/server/feed';
import { parseKillFilter } from '$lib/kills';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, event.locals, param(event, 'id'), 'server.view');
	const raw = event.url.searchParams.get('before');
	const before = raw ? new Date(raw) : null;
	if (before && Number.isNaN(before.getTime()))
		throw new ApiError(400, 'before must be an ISO timestamp.');
	const limit = int(event.url.searchParams.get('limit'), 50, 1, 200);
	const filter = parseKillFilter(event.url.searchParams);
	const [setup, kills, total] = await Promise.all([
		feedSetup(env, server, false),
		recentKills(env, server.id, before, limit, filter),
		event.url.searchParams.get('count') === '1' ? countKills(env, server.id, filter) : null
	]);
	return apiJson({ ok: true, configured: setup.configured, feedAt: setup.feedAt, kills, total });
});
