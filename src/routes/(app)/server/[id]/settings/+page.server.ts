import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { listWebhooks } from '$lib/server/webhooks';

/**
 * The Settings tab: the Discord channels that carry this server's status card or its team kills
 * (the org's webhooks that do either and cover this server), and the public pages. Webhook URLs
 * are org-owner territory, so everyone else sees a note. The public addresses are absolute so
 * they can be copied. Access goes through the server check, so a suspended org's owner is
 * refused here as on every other tab.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	let found;
	try {
		found = await requireServerCap(env, locals, params.id, 'server.view');
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
	const { server, access } = found;
	const https = env.ORIGIN.startsWith('https://');
	const origin = env.ORIGIN;
	if (!access.manager) return { owner: false, https, origin, channels: [] };
	const all = await listWebhooks(env, server.orgId);
	return {
		owner: true,
		https,
		origin,
		channels: all.filter(
			(w) =>
				(w.statusEnabled || w.events.includes('teamkills')) &&
				(!w.serverIds || w.serverIds.includes(server.id))
		)
	};
};
