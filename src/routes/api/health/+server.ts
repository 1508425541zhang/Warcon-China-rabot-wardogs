import type { RequestHandler } from './$types';
import { getEnv } from '$lib/server/env';
import { apiJson } from '$lib/server/http';
import { gateway } from '$lib/server/gateway';
import { timingSafeEqualStr } from '$lib/server/crypto';

// Liveness is public: a monitor or the container healthcheck reads only `ok`. The worker's
// scheduler stats are fleet-wide operational data — total servers and players across every
// organisation, memory and error rates — the same figures /metrics keeps behind a token, so they
// are added only for the site owner's own session or a caller presenting METRICS_TOKEN.
export const GET: RequestHandler = async ({ locals, request }) => {
	const env = getEnv();
	const token = env.METRICS_TOKEN;
	const privileged =
		locals.user?.role === 'owner' ||
		(!!token && timingSafeEqualStr(request.headers.get('authorization') || '', `Bearer ${token}`));
	if (!privileged) return apiJson({ ok: true, service: 'warcon' });
	const worker = await gateway()
		.health(env)
		.catch((err) => ({ enabled: false, error: err instanceof Error ? err.message : String(err) }));
	return apiJson({ ok: true, service: 'warcon', role: env.WARCON_ROLE, worker });
};
