import { pageLoad, pageActions } from '$lib/native/transport.server';

export const load = (event: Parameters<typeof pageLoad>[0]) =>
	pageLoad(event, 'src/routes/(auth)/sign-up/+page.server.ts');
export const actions = pageActions('src/routes/(auth)/sign-up/+page.server.ts', [
	'discord',
	'steam',
	'register',
	'create'
]);
