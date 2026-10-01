import { building } from '$app/environment';
import { redirect, type Handle, type HandleServerError } from '@sveltejs/kit';
import { nativeJson, nativeProxy } from '$lib/native/transport.server';
const HEADERS = {
	'x-content-type-options': 'nosniff',
	'x-frame-options': 'DENY',
	'referrer-policy': 'same-origin',
	'x-robots-tag': 'noindex, nofollow'
};
const ACCOUNT_PATH = /^\/(account|sign-out|join)(\/|$)/;
export const handle: Handle = async ({ event, resolve }) => {
	event.locals.user = null;
	event.locals.session = null;
	event.locals.apiKey = null;
	const path = event.url.pathname;
	if (
		!building &&
		(path.startsWith('/api/') || path === '/metrics' || path === '/auth/steam/callback')
	)
		return nativeProxy(event);
	if (!building) {
		const context = await nativeJson<{
			user: App.Locals['user'];
			session: App.Locals['session'];
			gate?: { password: boolean; enrolment: boolean };
		}>(event, '/api/identity/session');
		event.locals.user = context.user;
		event.locals.session = context.session;
		if (!ACCOUNT_PATH.test(path)) {
			if (context.gate?.password) redirect(303, '/account?force=1');
			if (context.gate?.enrolment) redirect(303, '/account?enrol=1');
		}
	}
	const response = await resolve(event);
	for (const [key, value] of Object.entries(HEADERS)) response.headers.set(key, value);
	return response;
};
export const handleError: HandleServerError = ({ status, message }) => ({
	message: status === 404 ? '未找到页面。' : message || '服务暂时不可用。'
});
