import { describe, expect, test } from 'bun:test';
import { SQL } from 'bun';
import { randomBytes } from 'node:crypto';
import { copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { sql } from 'drizzle-orm';
import { connect, migrationStatus, runMigrations } from '$lib/server/db';
import { hasTestDb } from './db';

describe.skipIf(!hasTestDb)('existing database upgrade', () => {
	test('0033 upgrades through latest and verifies schema and ordered history', async () => {
		const base = process.env.TEST_DATABASE_URL!;
		const name = `warcon_upgrade_${randomBytes(5).toString('hex')}`;
		const folder = await mkdtemp(join(tmpdir(), 'warcon-migration-'));
		const full = resolve(process.cwd(), 'drizzle');
		const admin = new SQL(base, { max: 1 });
		let connection: ReturnType<typeof connect> | null = null;
		try {
			await admin.unsafe(`CREATE DATABASE "${name}"`);
			const url = new URL(base);
			url.pathname = `/${name}`;
			connection = connect(url.href);
			const journal = JSON.parse(await Bun.file(join(full, 'meta', '_journal.json')).text()) as {
				entries: { idx: number; tag: string }[];
			};
			const old = { ...journal, entries: journal.entries.filter((entry) => entry.idx <= 33) };
			await mkdir(join(folder, 'meta'));
			await writeFile(join(folder, 'meta', '_journal.json'), JSON.stringify(old));
			for (const entry of old.entries)
				await copyFile(join(full, `${entry.tag}.sql`), join(folder, `${entry.tag}.sql`));
			await runMigrations(connection.db, folder);
			expect(await migrationStatus(connection.db, full)).toMatchObject({
				pending: journal.entries.length - old.entries.length,
				historyMismatch: false,
				applied: old.entries.length
			});
			// Reproduce a deployed 0041 database with a score whose source IDs were lost.
			const before42 = { ...journal, entries: journal.entries.filter((entry) => entry.idx <= 41) };
			await writeFile(join(folder, 'meta', '_journal.json'), JSON.stringify(before42));
			for (const entry of before42.entries.filter((entry) => entry.idx > 33))
				await copyFile(join(full, `${entry.tag}.sql`), join(folder, `${entry.tag}.sql`));
			await runMigrations(connection.db, folder);
			await connection.db.execute(sql`
				INSERT INTO integrity_scores (org_id, server_id, steam_id, scored_at, rule_version,
					score, level, breakdown, current_behavior_anomaly, source)
				VALUES ('legacy-org', 'legacy-server', '76561198000000001', now(), 1,
					30, 'ACTIVE_WATCH', '[]'::jsonb, true, 'window')`);
			await runMigrations(connection.db, full);
			expect(await migrationStatus(connection.db, full)).toMatchObject({
				pending: 0,
				historyMismatch: false,
				expected: journal.entries.length,
				applied: journal.entries.length
			});
			const tables = [
				'integrity_profiles',
				'integrity_weapon_map',
				'integrity_windows',
				'integrity_rules',
				'integrity_scores',
				'integrity_cases',
				'integrity_reports',
				'integrity_report_events',
				'integrity_case_events',
				'integrity_actions'
			];
			for (const table of tables) {
				const [row] = await connection.db.execute(sql`SELECT to_regclass(${table}) AS name`);
				expect(row.name).toBe(table);
			}
			const expectedColumns = [
				'window_id',
				'report_id',
				'source',
				'org_id',
				'server_id',
				'steam_id',
				'scored_at',
				'rule_version',
				'score',
				'level',
				'breakdown',
				'current_behavior_anomaly'
			];
			const rows = await connection.db.execute(sql`
				SELECT column_name FROM information_schema.columns
				WHERE table_name = 'integrity_scores'`);
			const names = new Set(rows.map((row) => row.column_name));
			for (const column of expectedColumns) expect(names.has(column)).toBe(true);
			const [archived] = await connection.db.execute(sql`
				SELECT COUNT(*)::int AS n FROM integrity_scores_orphaned_0042`);
			expect(archived.n).toBe(1);
			await connection.db.execute(sql`
				UPDATE drizzle.__drizzle_migrations SET created_at = created_at + 1
				WHERE id = (SELECT MAX(id) FROM drizzle.__drizzle_migrations)`);
			expect((await migrationStatus(connection.db, full)).historyMismatch).toBe(true);
		} finally {
			await connection?.client.close().catch(() => {});
			await admin.unsafe(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`).catch(() => {});
			await admin.close();
			await rm(folder, { recursive: true, force: true });
		}
	});
});
