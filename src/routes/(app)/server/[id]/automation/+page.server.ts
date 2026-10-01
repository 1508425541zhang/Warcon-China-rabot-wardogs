import { pageLoad } from '$lib/native/transport.server';

export const load = (event: Parameters<typeof pageLoad>[0]) =>
	pageLoad(event, 'src/routes/(app)/server/[id]/automation/+page.server.ts');
