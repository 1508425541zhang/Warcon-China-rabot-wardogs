import type { ServerLiveRow } from './db/schema';

/** Read the actual player list, not player_sessions (whose faction is heartbeat-delayed). */
export function rosterFactions(
	live: Pick<ServerLiveRow, 'players' | 'playersAt' | 'status' | 'statusAt'> | undefined,
	map: string
): Map<string, string> | null {
	if (
		!live?.playersAt ||
		!live.statusAt ||
		live.statusAt > live.playersAt ||
		live.playersAt.getTime() - live.statusAt.getTime() > 10_000 ||
		!Array.isArray(live.players)
	)
		return null;
	const status = live.status as { map?: unknown; scores?: unknown } | null;
	if (status?.map !== map) return null;
	const teams = Array.isArray(status.scores)
		? new Set(
				status.scores
					.map((score: unknown) => (score as { name?: unknown } | null)?.name)
					.filter((name: unknown): name is string => typeof name === 'string')
			)
		: null;
	const out = new Map<string, string>();
	for (const player of live.players) {
		if (!player || typeof player !== 'object') continue;
		const { steamId, faction } = player as { steamId?: unknown; faction?: unknown };
		if (
			typeof steamId !== 'string' ||
			typeof faction !== 'string' ||
			!faction ||
			faction === 'White' ||
			(teams?.size && !teams.has(faction))
		)
			continue;
		out.set(steamId, faction);
	}
	return out;
}
