import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { requireListsRole } from '$lib/server/access';
import { orgListsView } from '$lib/server/lists';

/** The org lists the caller edits, with entry counts, and their role on them (owner or editor). */
export const GET = route(async (event) => {
	const env = getEnv();
	const { org, role } = await requireListsRole(env, event.locals, param(event, 'id'), 'any');
	return apiJson({ ok: true, ...(await orgListsView(env, org, role)) });
});
