import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { getOrg, listsRoleFor, requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { orgListsView, serverListsState } from '$lib/server/lists';

/**
 * Checked here as well as in the server layout: a page's data can be asked for without its
 * layouts (SvelteKit's __data.json), so the layout's refusal protects nothing below it. Viewers
 * see the slots read-only.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server, user, access } = await requireServerCap(env, locals, params.id, 'server.view');
		const [listState, org, role] = await Promise.all([
			serverListsState(env, server, user, access),
			getOrg(env, server.orgId),
			listsRoleFor(env, user, server.orgId)
		]);
		return {
			listState,
			/** the org's lists with counts, for people who may open them */
			orgLists: org && role ? await orgListsView(env, org, role) : null
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
