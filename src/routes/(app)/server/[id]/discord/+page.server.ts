import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { getServer, orgRoleFor, requireUser } from '$lib/server/access';
import { listWebhooks } from '$lib/server/webhooks';

/**
 * The Discord channels that carry this server's status card or its team kills: the org's
 * webhooks that do either and cover this server. Webhook URLs are org-owner territory, so
 * everyone else sees a note.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	const user = requireUser(locals);
	const server = await getServer(env, params.id);
	const role = server && !user.apiKey ? await orgRoleFor(env, user, server.orgId) : null;
	const https = env.ORIGIN.startsWith('https://');
	if (!server || role !== 'owner') return { owner: false, https, channels: [] };
	const all = await listWebhooks(env, server.orgId);
	return {
		owner: true,
		https,
		channels: all.filter(
			(w) =>
				(w.statusEnabled || w.events.includes('teamkills')) &&
				(!w.serverIds || w.serverIds.includes(server.id))
		)
	};
};
