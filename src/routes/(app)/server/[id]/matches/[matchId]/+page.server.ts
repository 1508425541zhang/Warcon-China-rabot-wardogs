import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { loadMatch, parseMatchId } from '$lib/server/matches';

/**
 * Checked here as well as in the server layout: a page's data can be asked for without its
 * layouts (SvelteKit's __data.json), so the layout's refusal protects nothing below it. The match
 * must be this server's and have ended; anything else is not found.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server } = await requireServerCap(env, locals, params.id, 'server.view');
		const id = parseMatchId(params.matchId);
		const view = id === null ? null : await loadMatch(env, server.id, id);
		if (!view) error(404, 'No such match here.');
		return { match: view };
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
