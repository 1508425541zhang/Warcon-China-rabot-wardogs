import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { getServer } from '$lib/server/access';
import { steamEnabled } from '$lib/server/steam';
import { listTriggers } from '$lib/server/triggers';

/** The server layout already refused anyone without access; viewers see the rules read-only. */
export const load: PageServerLoad = async ({ params }) => {
	const env = getEnv();
	const [triggers, row] = await Promise.all([
		listTriggers(env, params.id),
		getServer(env, params.id)
	]);
	// What the kinds need before they can run here, so the Add menu and the editor can say so.
	return { triggers, steam: steamEnabled(env), feed: !!row?.feedTokenHash };
};
