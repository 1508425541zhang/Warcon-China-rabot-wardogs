import type { PageServerLoad } from './$types';
import { error } from '@sveltejs/kit';
import { getEnv } from '$lib/server/env';
import { servers, organizations } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { vipSettings } from '$lib/server/qq/vip';
import { qqSettingsView } from '$lib/server/qq/settings';

export const load: PageServerLoad = async ({ locals }) => {
	if (locals.user?.role !== 'owner' || locals.user.apiKey) error(403, 'Owner access required.');
	const env = getEnv();
	const [config, choices] = await Promise.all([
		qqSettingsView(env),
		env.db
			.select({ id: servers.id, name: servers.name, orgName: organizations.name })
			.from(servers)
			.innerJoin(organizations, eq(servers.orgId, organizations.id))
	]);
	return {
		config,
		vips: await vipSettings(env),
		servers: choices,
		callback: `${env.ORIGIN}/api/qq/webhook`
	};
};
