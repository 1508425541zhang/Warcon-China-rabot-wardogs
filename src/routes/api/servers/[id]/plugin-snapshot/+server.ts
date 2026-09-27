import { apiJson, param, route } from '$lib/server/http';
import { getEnv } from '$lib/server/env';
import { loadPluginSnapshot } from '$lib/server/personal-plugins';
/** Session or organization API key, both checked for server.view and server scope. */
export const GET = route(async (e) =>
	apiJson({ ok: true, snapshot: await loadPluginSnapshot(getEnv(), e.locals, param(e, 'id')) })
);
