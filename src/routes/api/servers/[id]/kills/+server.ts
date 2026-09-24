// The server's kill feed as stored: newest first, paged by `before` (an ISO timestamp) with
// `beforeTime` (that row's match-clock seconds, so a batch sharing a receipt time pages cleanly), narrowed
// by the filter in $lib/kills (killer, victim, player, cause, kind, minM), plus whether a feed is
// set up and when its last batch arrived. `count=1` adds how many kills match over the whole
// history, which the Kills tab wants once per filter and the Overview panel never. `match=<id>`
// narrows it to one match of the server (a match page's feed). Live updates come over the event
// stream (/api/live/events, event `kills`); this is a page's starting point and its history.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, int, param, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { countKills, feedSetup, recentKills } from '$lib/server/feed';
import { parseKillFilter } from '$lib/kills';
import { matchWindow, parseMatchId } from '$lib/server/matches';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, event.locals, param(event, 'id'), 'server.view');
	const raw = event.url.searchParams.get('before');
	const ts = raw ? new Date(raw) : null;
	if (ts && Number.isNaN(ts.getTime())) throw new ApiError(400, 'before must be an ISO timestamp.');
	const rawTime = event.url.searchParams.get('beforeTime');
	const eventTime = rawTime !== null && rawTime !== '' ? Number(rawTime) : null;
	if (eventTime !== null && !Number.isFinite(eventTime))
		throw new ApiError(400, 'beforeTime must be a number.');
	const before = ts ? { ts, eventTime } : null;
	const limit = int(event.url.searchParams.get('limit'), 50, 1, 200);
	const filter = parseKillFilter(event.url.searchParams);
	const rawMatch = event.url.searchParams.get('match');
	const matchId = rawMatch ? parseMatchId(rawMatch) : null;
	if (rawMatch && matchId === null) throw new ApiError(400, 'match must be a match id.');
	const match = matchId === null ? null : await matchWindow(env, server.id, matchId);
	if (matchId !== null && !match) throw new ApiError(404, 'No such match here.', 'not_found');
	const [setup, kills, total] = await Promise.all([
		feedSetup(env, server, false),
		recentKills(env, server.id, before, limit, filter, match),
		event.url.searchParams.get('count') === '1' ? countKills(env, server.id, filter, match) : null
	]);
	return apiJson({ ok: true, configured: setup.configured, feedAt: setup.feedAt, kills, total });
});
