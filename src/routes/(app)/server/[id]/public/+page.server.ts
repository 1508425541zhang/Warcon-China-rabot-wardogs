import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';

/** The public pages' addresses are absolute so they can be copied; the switches ride on the layout's server. */
export const load: PageServerLoad = async () => ({ origin: getEnv().ORIGIN });
