import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { listMembers } from '$lib/server/orgs';
import { listRoles } from '$lib/server/roles';

/** The access matrix: members by servers. Owners of the org (and the site owner) only. */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { org } = await requireOrgRole(env, locals, params.id, 'owner');
		const [members, roles] = await Promise.all([listMembers(env, org.id), listRoles(env, org.id)]);
		return { members, roles };
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
