// The Admin overview's poll (site owner only). ?recount=1 tallies the players seen again instead
// of serving the cached figure.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, route } from '$lib/server/http';
import { overview } from '$lib/server/overview';

export const GET = route(async ({ locals, url }) => {
	if (locals.user?.role !== 'owner') throw new ApiError(403, 'Owner access required.', 'forbidden');
	return apiJson({
		ok: true,
		overview: await overview(getEnv(), { recount: url.searchParams.get('recount') === '1' })
	});
});
