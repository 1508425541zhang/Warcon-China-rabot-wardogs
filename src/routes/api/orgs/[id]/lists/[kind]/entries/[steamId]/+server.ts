import { getEnv } from '$lib/server/env';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { requireListsRole } from '$lib/server/access';
import { parseKind, removeEntry, updateEntry } from '$lib/server/lists';

/** Changes the reason or the expiry of an entry on the org's list. */
export const PATCH = route(async (event) => {
	const env = getEnv();
	const kind = parseKind(param(event, 'kind'));
	const { org, user } = await requireListsRole(env, event.locals, param(event, 'id'), kind);
	const result = await updateEntry(
		env,
		event.request,
		user,
		org,
		null,
		kind,
		param(event, 'steamId'),
		await readJson(event.request)
	);
	return apiJson({ ok: true, ...result });
});

export const DELETE = route(async (event) => {
	const env = getEnv();
	const kind = parseKind(param(event, 'kind'));
	const { org, user } = await requireListsRole(env, event.locals, param(event, 'id'), kind);
	const result = await removeEntry(env, event.request, user, org, kind, param(event, 'steamId'));
	return apiJson({ ok: true, ...result });
});
