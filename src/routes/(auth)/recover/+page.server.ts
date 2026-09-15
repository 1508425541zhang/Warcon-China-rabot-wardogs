// Sign in with a recovery key: the last resort for someone who lost every other way in. The key is
// single-use; the account page then asks for new methods before anything else.
import { fail, redirect } from '@sveltejs/kit';
import type { Actions, PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { clientIp, normalizeError, str } from '$lib/server/http';
import { writeAudit } from '$lib/server/audit';
import { clearLoginFailures, loginLockSeconds, noteLoginFailure } from '$lib/server/access';
import { consumeRecoveryKey } from '$lib/server/recovery';
import { refreshAuthComplete } from '$lib/server/enrolment';

export const load: PageServerLoad = async ({ locals }) => {
	if (locals.user) redirect(303, '/account');
	return {};
};

export const actions: Actions = {
	default: async ({ request, locals }) => {
		const env = getEnv();
		const form = await request.formData();
		const username = str(form.get('username'), 32);
		const key = str(form.get('key'), 80);
		if (!username || !key)
			return fail(400, { error: 'Username and recovery key are required.', username });

		const keys = [`u:${username.toLowerCase()}`, `ip:${clientIp(request) || 'unknown'}`];
		const lock = await loginLockSeconds(env, keys);
		if (lock > 0) {
			await writeAudit(env, request, {
				category: 'auth',
				action: 'recovery',
				outcome: 'denied',
				target: username,
				message: `Locked out for ${lock}s`
			});
			return fail(429, {
				error: `Too many failed attempts. Try again in ${Math.ceil(lock / 60)} minute(s).`,
				username
			});
		}
		const row = await consumeRecoveryKey(env, username, key);
		if (!row || row.banned) {
			await noteLoginFailure(env, keys);
			await writeAudit(env, request, {
				category: 'auth',
				action: 'recovery',
				outcome: 'denied',
				target: username,
				message: row?.banned ? 'Account disabled' : 'Bad username or recovery key'
			});
			return fail(401, { error: 'That username and recovery key do not match.', username });
		}
		await clearLoginFailures(env, keys);
		await refreshAuthComplete(env, row.id);
		await writeAudit(env, request, {
			actor: { id: row.id, username: row.displayUsername ?? row.username ?? username },
			category: 'auth',
			action: 'recovery',
			outcome: 'ok',
			target: username,
			message: 'Signed in with the recovery key; the key is now used up'
		});
		try {
			await locals.auth!.api.signInUser({ body: { userId: row.id }, headers: request.headers });
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			return fail(known.status, { error: known.message, username });
		}
		redirect(303, '/account?recovered=1');
	}
};
