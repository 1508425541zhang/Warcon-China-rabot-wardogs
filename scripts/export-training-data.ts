/** Full, consistent source export; no model scores or fabricated reward labels. */
import { SQL } from 'bun';
import { mkdir, open, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';

const [serverId, destination] = process.argv.slice(2);
if (!serverId || !destination || !/^[a-zA-Z0-9_-]+$/.test(serverId)) {
	throw new Error(
		'Usage: bun scripts/export-training-data.ts SERVER_ID NEW_OUTPUT_DIRECTORY (DATABASE_URL or PG* environment required)'
	);
}
if (!process.env.DATABASE_URL && (!process.env.PGUSER || !process.env.PGDATABASE)) {
	throw new Error('Set DATABASE_URL or PGHOST/PGPORT/PGUSER/PGPASSWORD/PGDATABASE.');
}
const db = process.env.DATABASE_URL
	? new SQL(process.env.DATABASE_URL, { max: 1 })
	: new SQL({
			hostname: process.env.PGHOST ?? '127.0.0.1',
			port: Number(process.env.PGPORT ?? 5432),
			username: process.env.PGUSER,
			password: process.env.PGPASSWORD,
			database: process.env.PGDATABASE,
			max: 1
		});
const out = resolve(destination);
const files: { name: string; rows: number; sha256: string }[] = [];
try {
	await mkdir(out); // Refuse overwriting an earlier export.
	await db.begin(async (tx) => {
		await tx`SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY`;
		await tx`SET LOCAL TIME ZONE 'UTC'`;
		const [clock] = await tx`SELECT now() AS snapshot_at`;
		for (const table of ['training_feed_batches', 'training_observations'] as const) {
			const file = await open(join(out, table + '.jsonl'), 'wx');
			const hash = createHash('sha256');
			let lastId = '0';
			let count = 0;
			try {
				while (true) {
					// Table names are a fixed local allowlist. Return PostgreSQL JSON text directly:
					// this preserves numeric precision and UTC timestamptz values without JS rewriting.
					const rows = await tx.unsafe(
						`SELECT id::text AS cursor, row_to_json(t)::text AS record FROM ${table} t WHERE server_id=$1 AND id>$2::bigint ORDER BY id LIMIT 1000`,
						[serverId, lastId]
					);
					if (!rows.length) break;
					for (const row of rows) {
						const line = row.record + '\n';
						await file.writeFile(line);
						hash.update(line);
						lastId = row.cursor;
						count++;
					}
				}
			} finally {
				await file.close();
			}
			files.push({ name: table + '.jsonl', rows: count, sha256: hash.digest('hex') });
		}
		await writeFile(
			join(out, 'manifest.json'),
			JSON.stringify(
				{
					schemaVersion: 1,
					serverId,
					snapshotAt: clock.snapshot_at,
					files,
					mode: 'full_snapshot',
					deduplicateFeedEventsBy: ['server_id', 'instance_id', 'eventId'],
					note: 'Batch retries are preserved. Missing values remain missing. Source eventTime is a game-relative clock, not Unix time. No cheating truth or RL rewards are assigned.'
				},
				null,
				2
			) + '\n',
			{ flag: 'wx' }
		);
	});
	console.log(JSON.stringify({ output: out, files }));
} finally {
	await db.close();
}
