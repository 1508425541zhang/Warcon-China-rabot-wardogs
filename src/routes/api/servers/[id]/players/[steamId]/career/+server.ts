// A player's career across the organisation's servers the caller can see: rank, streak, results
// by map and faction, the last ten matches. Same auth as the dossier.
import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { accessibleServers, requireServerCap } from '$lib/server/access';
import { requireSteamId } from '$lib/server/steam';
import { loadCareer } from '$lib/server/leaderboards';

export const GET = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'server.view'
	);
	const steamId = requireSteamId(param(event, 'steamId'));
	const visible = (await accessibleServers(env, user, server.orgId)).filter(
		(s) => s.orgId === server.orgId
	);
	const career = await loadCareer(env, {
		serverId: server.id,
		ids: visible.map((s) => s.id),
		nameOf: new Map(visible.map((s) => [s.id, s.name])),
		steamId
	});
	return apiJson({ ok: true, career });
});
