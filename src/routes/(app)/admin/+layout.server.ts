import { error } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';

/** Everything under /admin is the site owner's. */
export const load: LayoutServerLoad = ({ locals }) => {
	if (locals.user?.role !== 'owner') error(403, 'Owner access required.');
	return {};
};
