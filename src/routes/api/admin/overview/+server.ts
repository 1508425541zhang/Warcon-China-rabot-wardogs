// The Admin overview's poll (site owner only). ?recount=1 tallies the players seen again instead
// of serving the cached figure.
import { getEnv } from '$lib/server/env';
import { requireOwner } from '$lib/server/access';
import { apiJson, route } from '$lib/server/http';
import { overview } from '$lib/server/overview';

export const GET = route(async ({ locals, url }) => {
	requireOwner(locals);
	return apiJson({
		ok: true,
		overview: await overview(getEnv(), { recount: url.searchParams.get('recount') === '1' })
	});
});
