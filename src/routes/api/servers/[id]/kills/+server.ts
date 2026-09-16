// The server's kill feed as stored: newest first, paged by `before` (an ISO timestamp), plus
// whether a feed is set up and when its last batch arrived. Live updates come over the event
// stream (/api/live/events, event `kills`); this is the page's starting point and its history.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, int, param, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { feedSetup, recentKills } from '$lib/server/feed';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, event.locals, param(event, 'id'), 'server.view');
	const raw = event.url.searchParams.get('before');
	const before = raw ? new Date(raw) : null;
	if (before && Number.isNaN(before.getTime()))
		throw new ApiError(400, 'before must be an ISO timestamp.');
	const limit = int(event.url.searchParams.get('limit'), 50, 1, 200);
	const [setup, kills] = await Promise.all([
		feedSetup(env, server, false),
		recentKills(env, server.id, before, limit)
	]);
	return apiJson({ ok: true, configured: setup.configured, feedAt: setup.feedAt, kills });
});
