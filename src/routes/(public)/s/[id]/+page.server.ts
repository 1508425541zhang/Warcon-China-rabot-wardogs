// The public live page of one server: 404 unless its status page is on. Reads the live snapshot
// row, never the worker.
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import {
	publicHeading,
	publicLoad,
	readPublicStatus,
	requirePublicServer
} from '$lib/server/public';

export const load: PageServerLoad = (event) =>
	publicLoad(event, async () => {
		const env = getEnv();
		const ps = await requirePublicServer(env, event.params.id, 'status');
		return { view: await readPublicStatus(env, ps), heading: publicHeading(ps) };
	});
