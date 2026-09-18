// Second step of a password sign-in for accounts with an authenticator app: the code (or a backup
// code). Better Auth left a signed challenge cookie on the browser at step one; without it there is
// nothing to verify and the visitor goes back to the sign-in page.
import { fail, redirect } from '@sveltejs/kit';
import type { Actions, PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { addressKey, normalizeError, str } from '$lib/server/http';
import { writeAudit } from '$lib/server/audit';
import { clearLoginFailures, loginLockSeconds, noteLoginFailure } from '$lib/server/access';

const CHALLENGE_COOKIES = ['warcon.two_factor', '__Secure-warcon.two_factor'];

const nextPath = (url: URL): string => {
	const next = url.searchParams.get('next') || '';
	return /^\/(?![/\\])[^\s\\]*$/.test(next) ? next : '/';
};

const hasChallenge = (cookies: { get: (n: string) => string | undefined }) =>
	CHALLENGE_COOKIES.some((c) => !!cookies.get(c));

export const load: PageServerLoad = async ({ locals, cookies, url }) => {
	if (locals.user) redirect(303, nextPath(url));
	if (!hasChallenge(cookies)) redirect(303, '/sign-in');
	return { next: nextPath(url) };
};

export const actions: Actions = {
	default: async ({ request, locals, cookies, url }) => {
		const env = getEnv();
		if (!hasChallenge(cookies)) redirect(303, '/sign-in');
		const form = await request.formData();
		const raw = str(form.get('code'), 40);
		const code = raw.replace(/\s+/g, '');
		const trustDevice = form.get('trust') === 'on';
		if (!code) return fail(400, { error: 'Enter the code from your authenticator app.' });

		// The username is not known here (only the challenge cookie is), so the throttle is per address;
		// Better Auth also locks the authenticator itself after repeated wrong codes.
		const keys = [`ip:${addressKey(request, env.BETTER_AUTH_SECRET ?? '')}`];
		const lock = await loginLockSeconds(env, keys);
		if (lock > 0)
			return fail(429, {
				error: `Too many failed attempts. Try again in ${Math.ceil(lock / 60)} minute(s).`
			});
		try {
			if (/^\d{6}$/.test(code))
				await locals.auth!.api.verifyTOTP({
					body: { code, trustDevice },
					headers: request.headers
				});
			else
				await locals.auth!.api.verifyBackupCode({
					body: { code, trustDevice },
					headers: request.headers
				});
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			await noteLoginFailure(env, keys);
			await writeAudit(env, request, {
				category: 'auth',
				action: 'login',
				outcome: 'denied',
				target: 'second factor',
				message: known.message
			});
			if (/INVALID_TWO_FACTOR_COOKIE|two factor cookie/i.test(known.message))
				redirect(303, '/sign-in?error=expired');
			return fail(401, { error: 'That code was not accepted.' });
		}
		await clearLoginFailures(env, keys);
		redirect(303, nextPath(url));
	}
};
