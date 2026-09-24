// Public status JSON for one server: what /s/<id> shows and polls. 404 unless the status page is
// on; rate limited per address; cacheable for a few seconds.
import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import {
	limitPublicReads,
	publicHeaders,
	readPublicStatus,
	requirePublicServer
} from '$lib/server/public';

export const GET = route(async (event) => {
	const env = getEnv();
	limitPublicReads(event.request);
	const ps = await requirePublicServer(env, param(event, 'id'), 'status');
	return apiJson({ ok: true, server: await readPublicStatus(env, ps) }, 200, publicHeaders(5));
});
