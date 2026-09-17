import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { accessibleServers, getServer, requireUser } from '$lib/server/access';
import { dossier } from '$lib/server/players';
import { loadCareer } from '$lib/server/leaderboards';
import { accessFromCaps } from '$lib/server/access-resolve';

export const load: PageServerLoad = async ({ locals, params, parent }) => {
	const env = getEnv();
	const user = requireUser(locals);
	// The server layout already refused anyone without access to this server.
	const [{ server }, row] = await Promise.all([parent(), getServer(env, params.id)]);
	if (!row) error(404, 'Server not found.');
	if (!/^\d{17}$/.test(params.steamId)) error(404, 'Not a SteamID64.');
	const visible = (await accessibleServers(env, user, row.orgId)).filter(
		(s) => s.orgId === row.orgId
	);
	const [d, career] = await Promise.all([
		dossier(env, user, row, accessFromCaps(server.caps, server.roleName), params.steamId),
		loadCareer(env, {
			serverId: row.id,
			ids: visible.map((s) => s.id),
			nameOf: new Map(visible.map((s) => [s.id, s.name])),
			steamId: params.steamId
		})
	]);
	return { dossier: d, career, multiServer: visible.length > 1 };
};
