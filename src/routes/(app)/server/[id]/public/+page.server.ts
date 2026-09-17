import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

/** Moved into the Settings tab; old links and bookmarks still land there. */
export const load: PageServerLoad = ({ params }) => {
	redirect(301, `/server/${encodeURIComponent(params.id)}/settings`);
};
