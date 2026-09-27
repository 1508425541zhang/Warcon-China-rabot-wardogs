import { getEnv } from '$lib/server/env';
import { accessibleServers } from '$lib/server/access';
import { listPersonalPlugins, pluginUser } from '$lib/server/personal-plugins';
import { codePluginCatalog } from '$lib/plugins/catalog';
import type { PageServerLoad } from './$types';
export const load: PageServerLoad = async ({ locals }) => {
	const env = getEnv();
	const user = pluginUser(locals);
	const [plugins, available] = await Promise.all([
		listPersonalPlugins(env, locals),
		accessibleServers(env, user)
	]);
	return {
		plugins,
		catalog: codePluginCatalog,
		pluginServers: available.map((s) => ({ id: s.id, name: s.name, orgName: s.orgName }))
	};
};
