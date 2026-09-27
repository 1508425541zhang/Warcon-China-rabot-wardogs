import { error } from '@sveltejs/kit';
import { getEnv } from '$lib/server/env';
import { getPersonalPlugin } from '$lib/server/personal-plugins';
import { normalizeError } from '$lib/server/http';
import type { PageServerLoad } from './$types';
export const load: PageServerLoad = async ({ locals, params }) => {
	try {
		return { plugin: await getPersonalPlugin(getEnv(), locals, params.pluginId) };
	} catch (err) {
		const known = normalizeError(err);
		if (known) error(known.status, known.message);
		throw err;
	}
};
