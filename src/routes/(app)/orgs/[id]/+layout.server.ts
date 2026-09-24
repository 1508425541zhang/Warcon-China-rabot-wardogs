import { error } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { accessibleServers, requireListsRole } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { listOrgs } from '$lib/server/orgs';
import { orgListsView } from '$lib/server/lists';

/**
 * The org section: owners see everything; someone whose role on one of its servers holds an org
 * list's capability opens that list, the org's servers and its players (each page applies its own
 * check on top: the overview is owners only, each list page wants its own list).
 */
export const load: LayoutServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { org, role, user } = await requireListsRole(env, locals, params.id, 'any');
		// This org's servers regardless of the header scope: managing an org must not depend on it.
		const [servers, [view], lists] = await Promise.all([
			accessibleServers(env, user, org.id),
			listOrgs(env, [org.id]),
			orgListsView(env, org, role)
		]);
		return {
			org: view,
			orgServers: servers,
			/** owner: runs the org; otherwise the lists their roles on its servers open */
			listsRole: role,
			lists
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
