import { pageLoad, pageActions } from '$lib/native/transport.server';

export const load = (event: Parameters<typeof pageLoad>[0]) =>
	pageLoad(event, 'src/routes/(app)/account/+page.server.ts');
export const actions = pageActions('src/routes/(app)/account/+page.server.ts', [
	'defaultOrg',
	'steam',
	'password',
	'removePassword',
	'totpStart',
	'totpConfirm',
	'totpDisable',
	'backupCodes',
	'recoveryKey',
	'recoveryKeyClear',
	'revoke',
	'linkDiscord',
	'unlinkDiscord',
	'linkSteam',
	'unlinkSteam',
	'deleteAccount'
]);
