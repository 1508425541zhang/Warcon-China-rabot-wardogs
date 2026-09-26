import { loadWeaponDistances } from '$lib/server/weapon-distance';
import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { accessibleServers, requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { dossier } from '$lib/server/players';
import { loadCareer } from '$lib/server/leaderboards';
import { loadPlayerIntegrity } from '$lib/server/integrity/player-view';

/**
 * Checked here as well as in the server layout: a page's data can be asked for without its layouts
 * (SvelteKit's __data.json), so the layout's refusal protects nothing below it — and a dossier is
 * the most sensitive thing on the server. The sibling tabs guard the same way.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server, access, user } = await requireServerCap(env, locals, params.id, 'server.view');
		if (!/^\d{17}$/.test(params.steamId)) error(404, 'Not a SteamID64.');
		const visible = (await accessibleServers(env, user, server.orgId)).filter(
			(s) => s.orgId === server.orgId
		);
		const [d, career] = await Promise.all([
			dossier(env, user, server, access, params.steamId),
			loadCareer(env, {
				serverId: server.id,
				ids: visible.map((s) => s.id),
				nameOf: new Map(visible.map((s) => [s.id, s.name])),
				steamId: params.steamId
			})
		]);
		const integrity = access.caps.has('integrity.view')
			? await loadPlayerIntegrity(env, server, params.steamId)
			: null;
		const weaponDistances = access.caps.has('integrity.view')
			? await loadWeaponDistances(env, server.id, params.steamId)
			: null;
		return { dossier: d, career, multiServer: visible.length > 1, integrity, weaponDistances };
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
