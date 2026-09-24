import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { publicHeading, publicLoad, requirePublicServer } from '$lib/server/public';

export const load: PageServerLoad = (event) =>
	publicLoad(event, async () => {
		const server = await requirePublicServer(getEnv(), event.params.id, 'status');
		return { heading: publicHeading(server), signedIn: !!event.locals.user };
	});
