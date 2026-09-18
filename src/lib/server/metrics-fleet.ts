// The fleet gauges on the web process: row counts of the small tables, read once per scrape and
// no more than every ten seconds however often Prometheus asks. One statement, six subqueries,
// each a count over a table of at most a few thousand rows.
import { sql } from 'drizzle-orm';
import type { Env } from './env';
import { fleet, registerCollector } from './metrics';

const MIN_INTERVAL_MS = 10_000;
export const FLEET_TABLES = [
	'organizations',
	'users',
	'servers',
	'org_members',
	'webhooks',
	'triggers'
] as const;

export function registerFleetCollector(env: Env): () => void {
	let readAt = 0;
	return registerCollector(async () => {
		const now = Date.now();
		if (now - readAt < MIN_INTERVAL_MS) return;
		readAt = now;
		const [row] = (await env.db.execute(sql`
			SELECT (SELECT count(*) FROM organizations)::int AS organizations,
			       (SELECT count(*) FROM "user")::int       AS users,
			       (SELECT count(*) FROM servers)::int      AS servers,
			       (SELECT count(*) FROM org_members)::int  AS org_members,
			       (SELECT count(*) FROM webhooks)::int     AS webhooks,
			       (SELECT count(*) FROM triggers)::int     AS triggers`)) as unknown as Record<
			(typeof FLEET_TABLES)[number],
			number
		>[];
		for (const table of FLEET_TABLES) fleet.set({ table }, row[table]);
	});
}
