import { redirect } from '@sveltejs/kit';

export const GET = () => redirect(308, '/docs/integrity-import');
