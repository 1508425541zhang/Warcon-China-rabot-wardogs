import { factionLockView } from '$lib/server/faction-lock';
import { weaponRestrictionView } from '$lib/server/weapon-restrictions';
import { serverLive } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import type { Status } from '$lib/types';
import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { steamEnabled } from '$lib/server/steam';
import { listTriggers } from '$lib/server/triggers';

/**
 * Checked here as well as in the server layout: a page's data can be asked for without its
 * layouts (SvelteKit's __data.json), so the layout's refusal protects nothing below it. Viewers
 * see the rules read-only.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server } = await requireServerCap(env, locals, params.id, 'automation.manage');
		const [factionLock, weaponRestriction, [live]] = await Promise.all([
			factionLockView(env, server.id),
			weaponRestrictionView(env, server.id),
			env.db
				.select({ status: serverLive.status })
				.from(serverLive)
				.where(eq(serverLive.serverId, server.id))
		]);
		// What the kinds need before they can run here, so the Add menu and the editor can say so.
		return {
			factionLock,
			weaponRestriction,
			teams: (live?.status as Status | null)?.scores?.map((s) => s.name) ?? [],
			triggers: await listTriggers(env, server.id),
			steam: steamEnabled(env),
			feed: !!server.feedTokenHash
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
