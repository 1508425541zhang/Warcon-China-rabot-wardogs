import { apiJson, param, route } from '$lib/server/http';
import { getEnv } from '$lib/server/env';
import {
	getPersonalPlugin,
	deletePersonalPlugin,
	readPluginJson,
	savePersonalPlugin
} from '$lib/server/personal-plugins';
export const GET = route(async (e) =>
	apiJson({ ok: true, plugin: await getPersonalPlugin(getEnv(), e.locals, param(e, 'pluginId')) })
);
export const PUT = route(async (e) =>
	apiJson({
		ok: true,
		plugin: await savePersonalPlugin(
			getEnv(),
			e.locals,
			await readPluginJson(e.request),
			param(e, 'pluginId')
		)
	})
);
export const DELETE = route(async (e) => {
	await deletePersonalPlugin(getEnv(), e.locals, param(e, 'pluginId'));
	return apiJson({ ok: true });
});
