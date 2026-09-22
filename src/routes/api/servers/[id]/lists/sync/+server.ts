import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { getOrg, requireServerCap } from '$lib/server/access';
import { ApiError } from '$lib/server/http';
import { gateway } from '$lib/server/gateway';
import { summaryOf } from '$lib/server/lists-sync';
import { CAPABILITY_INFO, LIST_CAPABILITY } from '$lib/capabilities';

/**
 * Pushes the organisation's lists to this server now (through the worker). Either org list's
 * capability here will do: a sync changes no entry, it only applies what the lists hold.
 */
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, access } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'server.view'
	);
	const caps = Object.values(LIST_CAPABILITY);
	if (!caps.some((c) => access.caps.has(c)))
		throw new ApiError(
			403,
			`This needs '${caps.map((c) => CAPABILITY_INFO[c].label).join("' or '")}' on ${server.name}; your role '${access.roleName}' includes neither.`,
			'forbidden'
		);
	const org = await getOrg(env, server.orgId);
	if (!org) throw new ApiError(404, 'Organisation not found.');
	const sync = summaryOf(await gateway().syncServer(env, server, org, 15_000));
	return apiJson({ ok: true, sync });
});
