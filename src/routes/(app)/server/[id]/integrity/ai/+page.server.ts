import { getEnv } from '$lib/server/env';
import { requireServerCap, orgRoleFor } from '$lib/server/access';
import { aiSettings } from '$lib/server/integrity/ai';
import { integrityCases, steamProfiles } from '$lib/server/db/schema';
import { desc, eq } from 'drizzle-orm';
export const load = async ({ locals, params }: import('./$types').PageServerLoadEvent) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(env, locals, params.id, 'integrity.view');
	const config = await aiSettings(env, server.orgId);
	const cases = await env.db
		.select({
			name: steamProfiles.persona,
			id: integrityCases.id,
			steamId: integrityCases.steamId,
			createdAt: integrityCases.createdAt,
			status: integrityCases.status
		})
		.from(integrityCases)
		.leftJoin(steamProfiles, eq(steamProfiles.steamId, integrityCases.steamId))
		.where(eq(integrityCases.serverId, server.id))
		.orderBy(desc(integrityCases.createdAt))
		.limit(100);
	return {
		canConfigure: (await orgRoleFor(env, user, server.orgId)) === 'owner',
		config: config
			? {
					baseUrl: config.baseUrl,
					model: config.model,
					maxTokens: config.maxTokens,
					tokenParameter: config.tokenParameter,
					hasKey: true
				}
			: null,
		cases
	};
};
