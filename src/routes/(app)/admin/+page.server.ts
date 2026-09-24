import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { overview } from '$lib/server/overview';

// Checked here as well as in the admin layout: a page's data can be asked for without its layouts.
export const load: PageServerLoad = async ({ locals }) => {
	if (locals.user?.role !== 'owner') error(403, 'Owner access required.');
	return { overview: await overview(getEnv()) };
};
