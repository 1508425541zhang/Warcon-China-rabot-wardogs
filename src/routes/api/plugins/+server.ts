import { apiJson, route } from '$lib/server/http';
import { getEnv } from '$lib/server/env';
import {
	listPersonalPlugins,
	readPluginJson,
	savePersonalPlugin
} from '$lib/server/personal-plugins';
export const GET = route(async ({ locals }) =>
	apiJson({ ok: true, plugins: await listPersonalPlugins(getEnv(), locals) })
);
export const POST = route(async ({ locals, request }) =>
	apiJson(
		{ ok: true, plugin: await savePersonalPlugin(getEnv(), locals, await readPluginJson(request)) },
		201
	)
);
