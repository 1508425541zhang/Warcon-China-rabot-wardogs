import { pageLoad, pageActions } from '$lib/native/transport.server';

export const load = (event: Parameters<typeof pageLoad>[0]) =>
	pageLoad(event, 'src/routes/(auth)/sign-in/verify/+page.server.ts');
export const actions = pageActions('src/routes/(auth)/sign-in/verify/+page.server.ts', ['default']);
