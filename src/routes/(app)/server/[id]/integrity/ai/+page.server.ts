import { aiJobViews } from '$lib/server/integrity/ai-queue';
import { SYSTEM, PROMPT_VERSION } from '$lib/server/integrity/ai-protocol';
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
		jobs: await aiJobViews(
			env,
			cases.map((c) => c.id)
		),
		prompt: SYSTEM,
		promptVersion: PROMPT_VERSION,
		canConfigure: (await orgRoleFor(env, user, server.orgId)) === 'owner',
		config: config
			? {
					autoEnabled: config.autoEnabled,
					dailyLimit: config.dailyLimit,
					dailyRequests:
						config.budgetDay === new Date().toISOString().slice(0, 10) ? config.dailyRequests : 0,
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
