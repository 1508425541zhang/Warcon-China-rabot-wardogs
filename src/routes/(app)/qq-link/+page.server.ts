import { pageLoad, pageActions } from '$lib/native/transport.server';

export const load = (event: Parameters<typeof pageLoad>[0]) =>
	pageLoad(event, 'src/routes/(app)/qq-link/+page.server.ts');
export const actions = pageActions('src/routes/(app)/qq-link/+page.server.ts', ['bind', 'unbind']);
