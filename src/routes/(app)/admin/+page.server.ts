import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { overview } from '$lib/server/overview';

export const load: PageServerLoad = async () => ({ overview: await overview(getEnv()) });
