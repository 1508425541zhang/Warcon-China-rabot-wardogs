// Step one of signing in with a passkey: a challenge (kept server-side, referenced by a cookie).
import { apiJson, clientIp, route } from '$lib/server/http';
import { assertRate } from '$lib/server/ratelimit';

export const POST = route(async ({ locals, request }) => {
	assertRate(`passkey-auth:${clientIp(request) || 'unknown'}`, 30, 60_000);
	const options = await locals.auth!.api.generatePasskeyAuthenticationOptions({
		headers: request.headers
	});
	return apiJson(options);
});
