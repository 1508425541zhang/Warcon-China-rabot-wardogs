import { sql } from 'drizzle-orm';
import type { Env } from './env';
import { isOwner } from './leadership';
import { summarizePlaytime } from './playtime-distribution';
export const WARDOGS_APP_ID = 1867240;
export function parsePlaytime(body: unknown): number | null {
	const games = (body as { response?: { games?: unknown } })?.response?.games;
	if (!Array.isArray(games)) return null;
	const game = games.find((g) => g?.appid === WARDOGS_APP_ID);
	const minutes = game?.playtime_forever;
	return typeof minutes === 'number' &&
		Number.isSafeInteger(minutes) &&
		minutes >= 0 &&
		minutes <= 2147483647
		? minutes
		: null;
}
export async function loadPlaytimeDistribution(env: Env, serverId: string, from: Date) {
	const rows = await env.db.execute(sql`
  WITH cohort AS (SELECT DISTINCT steam_id FROM player_sessions WHERE server_id = ${serverId}
   AND last_seen >= ${from.toISOString()} AND steam_id ~ '^[0-9]{17}$'),
  samples AS (SELECT CASE WHEN p.checked_at >= now() - interval '24 hours' THEN p.minutes ELSE NULL END AS minutes,
   CASE WHEN p.state = 'error' THEN 'error' WHEN p.checked_at >= now() - interval '24 hours' THEN p.state ELSE 'pending' END AS state
   FROM cohort c LEFT JOIN steam_game_playtime p USING (steam_id))
  SELECT minutes, state, count(*)::int AS count FROM samples GROUP BY minutes, state`);
	return summarizePlaytime(
		rows.map((r) => ({
			minutes: r.minutes === null ? null : Number(r.minutes),
			state: String(r.state),
			count: Number(r.count)
		})),
		Boolean(env.STEAM_API_KEY)
	);
}
let timer: ReturnType<typeof setInterval> | null = null;
let running: Promise<void> | null = null;
let enqueueAt = 0;
let backoffUntil = 0;
async function pass(env: Env) {
	if (!isOwner() || !env.STEAM_API_KEY || Date.now() < backoffUntil) return;
	if (Date.now() >= enqueueAt) {
		await env.db.execute(sql`INSERT INTO steam_game_playtime (steam_id)
   SELECT DISTINCT steam_id FROM player_sessions WHERE last_seen >= now() - interval '30 days' AND steam_id ~ '^[0-9]{17}$'
   ON CONFLICT DO NOTHING`);
		enqueueAt = Date.now() + 60000;
	}
	const jobs = await env.db
		.execute(sql`UPDATE steam_game_playtime SET lease_until = now() + interval '1 minute'
  WHERE steam_id IN (SELECT p.steam_id FROM steam_game_playtime p WHERE next_at <= now()
   AND (lease_until IS NULL OR lease_until < now())
   AND EXISTS (SELECT 1 FROM player_sessions s WHERE s.steam_id = p.steam_id AND s.last_seen >= now() - interval '30 days')
   ORDER BY next_at LIMIT 2 FOR UPDATE SKIP LOCKED) RETURNING steam_id`);
	await Promise.all(
		jobs.map(async (job) => {
			let minutes: number | null = null,
				state = 'error';
			try {
				const url = new URL('https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/');
				url.searchParams.set('key', env.STEAM_API_KEY!);
				url.searchParams.set(
					'input_json',
					JSON.stringify({
						steamid: String(job.steam_id),
						appids_filter: [WARDOGS_APP_ID],
						include_appinfo: false,
						include_played_free_games: true
					})
				);
				const response = await fetch(url, { signal: AbortSignal.timeout(5000) });
				if ([401, 403, 429].includes(response.status)) backoffUntil = Date.now() + 300000;
				if (!response.ok) throw new Error('Steam request failed');
				minutes = parsePlaytime(await response.json());
				state = minutes === null ? 'unavailable' : 'known';
			} catch {
				/* Never log the request URL: it contains the API key. */
			}
			await env.db
				.execute(sql`UPDATE steam_game_playtime SET minutes = ${minutes}, state = ${state}, checked_at = now(),
   next_at = now() + ${state === 'error' ? '1 hour' : '24 hours'}::interval, lease_until = NULL WHERE steam_id = ${String(job.steam_id)}`);
		})
	);
}
export function startSteamPlaytime(env: Env) {
	if (timer) clearInterval(timer);
	const tick = () => {
		if (running) return;
		running = pass(env)
			.catch(() => console.error('[warcon] Steam playtime refresh failed'))
			.finally(() => {
				running = null;
			});
	};
	timer = setInterval(tick, 5000);
	tick();
}
export async function stopSteamPlaytime() {
	if (timer) clearInterval(timer);
	timer = null;
	await running;
}
