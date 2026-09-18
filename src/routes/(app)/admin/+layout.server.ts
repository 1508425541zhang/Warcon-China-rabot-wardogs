import { error } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';

/**
 * Everything under /admin is the site owner's. This refusal is for the shell only: each page's
 * load checks the owner itself, because a page's data can be asked for without its layouts.
 */
export const load: LayoutServerLoad = ({ locals }) => {
	if (locals.user?.role !== 'owner') error(403, 'Owner access required.');
	return {};
};
