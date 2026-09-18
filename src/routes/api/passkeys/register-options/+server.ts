// Step one of registering a passkey. Signed in: for your own account. Not signed in: a
// passkey-first sign-up, allowed in the same places a password sign-up is (first-run setup, an
// invite link, or open organisation sign-up), with the same throttle and Turnstile check.
import { getEnv } from '$lib/server/env';
import { apiJson, ApiError, clientIp, readJson, route, str } from '$lib/server/http';
import { timingSafeEqualStr } from '$lib/server/crypto';
import { writeAudit } from '$lib/server/audit';
import { loginLockSeconds, noteLoginFailure } from '$lib/server/access';
import { userCount, validateUsername } from '$lib/server/users';
import { orgSignupEnabled, signupKeys, verifyTurnstile } from '$lib/server/signup';
import { findInvite, inviteProblem, suspendedProblem } from '$lib/server/orgs';
import type { PasskeySignup } from '$lib/server/auth';
import { and, eq, or } from 'drizzle-orm';
import { user } from '$lib/server/db/schema';
import { emailFor } from '$lib/server/auth';

export const POST = route(async ({ locals, request }) => {
	const env = getEnv();
	const auth = locals.auth!;
	const body = await readJson(request);
	const name = str(body.name, 60) || undefined;

	if (locals.user) {
		const options = await auth.api.generatePasskeyRegistrationOptions({
			query: { name },
			headers: request.headers
		});
		return apiJson(options);
	}

	// Sign-up. Who may create an account here mirrors registerFromForm in signup.ts.
	const keys = signupKeys(request, env);
	const lock = await loginLockSeconds(env, keys);
	if (lock > 0)
		throw new ApiError(
			429,
			`Too many sign-ups from your address. Try again in ${Math.ceil(lock / 60)} minute(s).`
		);
	const username = validateUsername(body.username);
	const displayName = str(body.displayName, 80);
	const signup: PasskeySignup = { username, name: displayName || username, role: 'member' };

	if ((await userCount(env)) === 0) {
		// First run: the owner. The setup token guards it exactly as /setup does.
		if (env.SETUP_TOKEN && !timingSafeEqualStr(str(body.token, 500), env.SETUP_TOKEN)) {
			await writeAudit(env, request, {
				category: 'auth',
				action: 'setup',
				outcome: 'denied',
				message: 'Bad setup token'
			});
			throw new ApiError(403, 'Setup token is wrong.');
		}
		signup.role = 'owner';
		signup.setup = true;
	} else {
		const invite = str(body.invite, 200);
		if (invite) {
			const found = await findInvite(env, invite);
			if (!found || inviteProblem(found.invite) || suspendedProblem(found.org))
				throw new ApiError(410, 'This invite link can no longer be used.');
		} else if (!orgSignupEnabled(env)) {
			throw new ApiError(404, 'Sign-up is not enabled.');
		}
		await noteLoginFailure(env, keys);
		const challenge = await verifyTurnstile(env, str(body.turnstile, 4000), clientIp(request));
		if (challenge) {
			await writeAudit(env, request, {
				category: 'auth',
				action: 'signup',
				outcome: 'denied',
				target: username,
				message: `Turnstile: ${challenge}`
			});
			throw new ApiError(403, challenge);
		}
	}
	const [taken] = await env.db
		.select({ id: user.id })
		.from(user)
		.where(and(or(eq(user.username, username.toLowerCase()), eq(user.email, emailFor(username)))))
		.limit(1);
	if (taken) throw new ApiError(409, 'That username is taken.');

	const options = await auth.api.generatePasskeyRegistrationOptions({
		query: { name, context: JSON.stringify(signup) },
		headers: request.headers
	});
	return apiJson(options);
});
