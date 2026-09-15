// GET: the signed-in user's passkeys. POST: step two of registration (see register-options): the
// browser's attestation goes to Better Auth, which stores the credential and, for a sign-up,
// creates the account and its session in the same step.
import { getEnv } from '$lib/server/env';
import { apiJson, ApiError, readJson, route, str } from '$lib/server/http';
import { requireUser } from '$lib/server/access';
import { writeAudit } from '$lib/server/audit';
import { listPasskeys } from '$lib/server/users';
import { refreshAuthComplete } from '$lib/server/enrolment';

export const GET = route(async ({ locals }) => {
	const env = getEnv();
	const user = requireUser(locals);
	return apiJson({ passkeys: await listPasskeys(env, user.id) });
});

export const POST = route(async ({ locals, request }) => {
	const env = getEnv();
	const auth = locals.auth!;
	const body = await readJson(request);
	if (!body.response || typeof body.response !== 'object')
		throw new ApiError(400, 'Missing passkey response.');
	const name = str(body.name, 60) || undefined;
	const signup = !locals.user;
	const result = await auth.api.verifyPasskeyRegistration({
		body: { response: body.response, name, createSession: signup },
		headers: request.headers
	});
	const userId = String((result as { userId?: string }).userId ?? locals.user?.id ?? '');
	const passkeyId = String((result as { id?: string }).id ?? '');
	if (!userId) throw new ApiError(500, 'Passkey was not stored.');
	await refreshAuthComplete(env, userId);
	if (signup) {
		const created = (result as { user?: { username?: string; displayUsername?: string } }).user;
		await writeAudit(env, request, {
			actor: { id: userId, username: created?.displayUsername ?? created?.username ?? '' },
			category: 'auth',
			action: 'signup',
			outcome: 'ok',
			target: created?.displayUsername ?? created?.username ?? '',
			message: 'Account created with a passkey'
		});
	} else {
		await writeAudit(env, request, {
			actor: locals.user!,
			category: 'auth',
			action: 'passkey.add',
			outcome: 'ok',
			target: name ?? 'Passkey'
		});
	}
	return apiJson({ ok: true, id: passkeyId, userId });
});
