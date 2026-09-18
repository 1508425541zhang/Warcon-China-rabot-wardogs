import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

/** Users is a tab of the Admin page now. */
export const load: PageServerLoad = () => {
	redirect(301, '/admin/users');
};
