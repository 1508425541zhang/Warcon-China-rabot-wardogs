// Postgres through Bun's built-in client, wrapped by Drizzle. Migrations from ./drizzle are applied
// on startup; TimescaleDB (optional but recommended) turns `samples` into a hypertable.
import { SQL } from 'bun';
import { sql } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/bun-sql';
import { migrate } from 'drizzle-orm/bun-sql/migrator';
import * as schema from './schema';

export { schema };

/** Opens the pool from a connection URL or from separate fields (host, user, password, database). */
export function connect(target: string | Bun.SQL.PostgresOrMySQLOptions) {
	let client: SQL;
	try {
		client =
			typeof target === 'string' ? new SQL(target, { max: 10 }) : new SQL({ ...target, max: 10 });
	} catch (err) {
		// Bun rejects the URL before connecting when the password holds / # % or ? unencoded.
		throw new Error(
			`DATABASE_URL is not a valid URL (${err instanceof Error ? err.message : String(err)}). Percent-encode the password, or pass PGHOST, PGUSER, PGPASSWORD and PGDATABASE instead.`,
			{ cause: err }
		);
	}
	const db = drizzle({ client, schema, casing: 'snake_case' });
	return { client, db };
}

export type Db = ReturnType<typeof connect>['db'];
export type SqlClient = ReturnType<typeof connect>['client'];
/** A Drizzle transaction handle, or the plain db when no transaction is open. */
export type Tx = Parameters<Parameters<Db['transaction']>[0]>[0];
export type DbOrTx = Db | Tx;

export async function runMigrations(db: Db, migrationsFolder: string): Promise<void> {
	await migrate(db, { migrationsFolder });
}

/** Compare the ordered migration history, not just the row count. */
export async function migrationStatus(
	db: Db,
	migrationsFolder: string
): Promise<{
	pending: number;
	historyMismatch: boolean;
	expected: number;
	applied: number;
}> {
	const journal = JSON.parse(await Bun.file(`${migrationsFolder}/meta/_journal.json`).text()) as {
		entries: { tag: string; when: number }[];
	};
	let rows: { created_at: string | number }[] = [];
	try {
		rows = (await db.execute(
			sql`SELECT created_at FROM drizzle.__drizzle_migrations ORDER BY id`
		)) as { created_at: string | number }[];
	} catch (err) {
		// A fresh database has no Drizzle schema. All other failures must remain visible.
		let cause: unknown = err;
		let missingTable = false;
		while (cause && typeof cause === 'object') {
			if ('errno' in cause && cause.errno === '42P01') missingTable = true;
			if ('code' in cause && cause.code === '42P01') missingTable = true;
			cause = 'cause' in cause ? cause.cause : null;
		}
		if (!missingTable) throw err;
	}
	const expected = journal.entries.length;
	const applied = rows.length;
	return {
		pending: Math.max(0, expected - applied),
		historyMismatch:
			applied > expected ||
			rows.some((row, index) => Number(row.created_at) !== journal.entries[index]?.when),
		expected,
		applied
	};
}

export async function pendingMigrations(db: Db, migrationsFolder: string): Promise<number> {
	return (await migrationStatus(db, migrationsFolder)).pending;
}

/** True when the timescaledb extension is installed in this database. */
export async function hasTimescale(db: Db): Promise<boolean> {
	const [row] = await db.execute<{ n: number }>(
		sql`SELECT COUNT(*)::int AS n FROM pg_extension WHERE extname = 'timescaledb'`
	);
	return (row?.n ?? 0) > 0;
}
