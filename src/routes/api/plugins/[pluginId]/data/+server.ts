import { ApiError, apiJson, param, route } from '$lib/server/http';
import { getEnv } from '$lib/server/env';
import { getPersonalPlugin, loadPluginSnapshot } from '$lib/server/personal-plugins';
export const GET = route(async e => {
 const env = getEnv(); const plugin = await getPersonalPlugin(env, e.locals, param(e, 'pluginId'));
 if (!plugin.enabled) throw new ApiError(409, '插件已停用。', 'plugin_disabled');
 return apiJson({ ok: true, apiVersion: 1, snapshot: plugin.serverId ? await loadPluginSnapshot(env, e.locals, plugin.serverId) : null });
});
