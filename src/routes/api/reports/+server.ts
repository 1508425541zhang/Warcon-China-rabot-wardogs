import { requireUser } from '$lib/server/access';
import { getEnv } from '$lib/server/env';
import { apiJson, readJson, route } from '$lib/server/http';
import { submitReport } from '$lib/server/integrity/reports';

/** Steam-verified community members may submit evidence for staff review. */
export const POST = route(async (event) => {
	const actor = requireUser(event.locals);
	const input = await readJson<{ serverId?: unknown; target?: unknown; reason?: unknown }>(
		event.request
	);
	const report = await submitReport(getEnv(), event.request, actor, {
		serverId: input.serverId,
		target: input.target,
		reason: input.reason
	});
	return apiJson({ ok: true, report }, 201);
});
