import { eq, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityProfileRefreshJobs } from '../db/schema';
import { getProfiles, steamEnabled } from '../steam';
import { isOwner } from '../leadership';

const HOUR = 3_600_000;
let timer: ReturnType<typeof setInterval> | null = null;
let running: Promise<void> | null = null;

export async function enqueueIntegrityProfileRefresh(
	env: Env,
	steamIds: readonly string[]
): Promise<void> {
	if (!steamEnabled(env)) return;
	for (const steamId of new Set(steamIds)) {
		if (!/^\d{17}$/.test(steamId)) continue;
		await env.db
			.insert(integrityProfileRefreshJobs)
			.values({ steamId })
			.onConflictDoUpdate({
				target: integrityProfileRefreshJobs.steamId,
				set: {
					state: sql`CASE WHEN ${integrityProfileRefreshJobs.updatedAt} < now() - interval '24 hours'
					THEN 'pending' ELSE ${integrityProfileRefreshJobs.state} END`,
					nextAt: sql`CASE WHEN ${integrityProfileRefreshJobs.updatedAt} < now() - interval '24 hours'
					THEN now() ELSE ${integrityProfileRefreshJobs.nextAt} END`
				}
			});
	}
}

export async function processNextIntegrityProfileRefresh(env: Env): Promise<boolean> {
	if (!isOwner() || !steamEnabled(env)) return false;
	const [job] = await env.db.execute(sql`
		UPDATE integrity_profile_refresh_jobs SET state = 'processing', attempts = attempts + 1,
		lease_until = now() + interval '2 minutes'
		WHERE steam_id = (SELECT steam_id FROM integrity_profile_refresh_jobs
			WHERE next_at <= now() AND (state = 'pending' OR (state = 'processing' AND lease_until <= now()))
			ORDER BY next_at LIMIT 1 FOR UPDATE SKIP LOCKED)
		RETURNING steam_id AS "steamId", attempts
	`);
	if (!job) return false;
	const steamId = String(job.steamId);
	try {
		await getProfiles(env, [steamId], { refresh: true, skipFriends: true });
		await env.db
			.update(integrityProfileRefreshJobs)
			.set({ state: 'done', leaseUntil: null, lastError: null, updatedAt: new Date() })
			.where(eq(integrityProfileRefreshJobs.steamId, steamId));
	} catch (error) {
		const backoff = Math.min(24 * HOUR, 30_000 * 2 ** Math.min(10, Number(job.attempts)));
		await env.db
			.update(integrityProfileRefreshJobs)
			.set({
				state: 'pending',
				leaseUntil: null,
				nextAt: new Date(Date.now() + backoff),
				lastError: String(error).slice(0, 500),
				updatedAt: new Date()
			})
			.where(eq(integrityProfileRefreshJobs.steamId, steamId));
	}
	return true;
}

export function startIntegrityProfileRefresh(env: Env): void {
	if (timer) clearInterval(timer);
	const pass = () => {
		if (running) return;
		running = (async () => {
			for (let i = 0; i < 10; i++) if (!(await processNextIntegrityProfileRefresh(env))) break;
		})()
			.catch((error) => console.error('[warcon] Steam enrichment queue:', error))
			.finally(() => {
				running = null;
			});
	};
	timer = setInterval(pass, 5_000);
	pass();
}
export async function stopIntegrityProfileRefresh(): Promise<void> {
	if (timer) clearInterval(timer);
	timer = null;
	await running;
}
