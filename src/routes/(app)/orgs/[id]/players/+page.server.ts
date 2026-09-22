import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { accessibleServers, requireListsRole } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { seenFilters, seenPlayers } from '$lib/server/seen';

const PAGE_SIZE = 100;

export const load: PageServerLoad = async ({ locals, params, url }) => {
	const env = getEnv();
	try {
		const { org, user } = await requireListsRole(env, locals, params.id, 'any');
		const serverIds = (await accessibleServers(env, user, org.id)).map((s) => s.id);
		const filters = seenFilters(url.searchParams);
		const { players, total } = await seenPlayers(env, {
			orgId: org.id,
			serverIds,
			filters,
			limit: PAGE_SIZE,
			offset: 0,
			steam: true
		});
		return { players, total, filters, pageSize: PAGE_SIZE };
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
