// The outside security review of 2026-09-19, finding by finding, for the ones no other suite
// already answers. A test named "by design" states what the reviewer saw and the panel means.
//
// Answered elsewhere: the config document (actions.test.ts "config hides the RCON password…",
// "raw does not serve the config document…", action-matrix for the capability), DNS rebinding
// (transport.test.ts "connects to the pinned address…"), page loads and form actions that lean
// on a layout (routes/page-loads.test.ts, load-matrix).
import { beforeAll, describe, expect, mock, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { kills, playerSessions, servers, twoFactor } from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { gateway } from '$lib/server/gateway';
import { callApi, stubGateway } from './call';
import { seedWorld, type World } from './world';

// Better Auth's cookie plugin asks for the request event after a sign-up; setup.ts throws there.
mock.module('$app/server', () => ({ getRequestEvent: () => ({ cookies: { set: () => {} } }) }));

const ROOT = join(import.meta.dir, '..', '..');
const ROUTES = join(ROOT, 'src', 'routes');
const read = (...p: string[]) => readFileSync(join(ROOT, ...p), 'utf8');

const ME = '76561198000000101';
const VICTIM = '76561198000000102';

describe('outside review: no database needed', () => {
	test('the .env.example RELAY_SECRET does not pass the boot gate of a split role', async () => {
		const example = /^RELAY_SECRET=(\S+)/m.exec(read('.env.example'))![1];
		const { initEnv } = await import('$lib/server/env');
		const before = { ...process.env };
		Object.assign(process.env, {
			ORIGIN: 'http://localhost:5173',
			BETTER_AUTH_SECRET: 'x'.repeat(44),
			RELAY_SECRET: example
		});
		delete process.env.RELAY_URL;
		try {
			await expect(initEnv({ role: 'web' })).rejects.toThrow(
				/RELAY_SECRET is still the placeholder/
			);
			// A real secret gets as far as the next check, so the refusal above is the placeholder's.
			process.env.RELAY_SECRET = 'r'.repeat(48);
			await expect(initEnv({ role: 'web' })).rejects.toThrow(/RELAY_URL/);
		} finally {
			for (const k of Object.keys(process.env)) if (!(k in before)) delete process.env[k];
			Object.assign(process.env, before);
		}
	});

	test('the relay and metrics bearers are compared in constant time', () => {
		for (const file of [
			'src/routes/api/health/+server.ts',
			'src/lib/server/metrics.ts',
			'src/worker/runtime.ts'
		]) {
			const src = read(file);
			expect({ file, constantTime: src.includes('timingSafeEqualStr') }).toEqual({
				file,
				constantTime: true
			});
			expect(src).not.toMatch(/(RELAY_SECRET|METRICS_TOKEN)[^\n]*[!=]==/);
		}
	});

	test('the bundled database publishes no port, so its default password is not reachable', () => {
		const compose = read('docker-compose.yml');
		const db = compose.slice(compose.search(/^ {2}db:/m));
		const service = db.slice(0, db.slice(1).search(/^ {2}\S/m) + 1 || undefined);
		expect(service).toContain('POSTGRES_PASSWORD');
		expect(service).not.toMatch(/^\s+ports:/m);
	});
});

describe.skipIf(!hasTestDb)('outside review: against the database', () => {
	let env: Env;
	let w: World;

	const get = async (path: string, who: keyof World['users'] | null, query = '', headers = {}) => {
		const { GET } = await import(join(ROUTES, path, '+server.ts'));
		const user = who ? w.users[who] : null;
		if (Object.keys(headers).length) {
			const res: Response = await GET({
				locals: { user, session: null, apiKey: null },
				request: new Request('http://localhost:5173/test', { headers })
			} as never);
			return { status: res.status, code: '', message: '', body: await res.json() };
		}
		return callApi(GET, user, { params: { id: w.server.id, steamId: ME }, query });
	};

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		stubGateway();
		gateway().health = async () => ({ enabled: true, servers: 412, players: 9000 }) as never;
		await env.db
			.update(servers)
			.set({ publicLeaderboards: true })
			.where(eq(servers.id, w.server.id));
		const at = new Date(Date.now() - 3600_000);
		await env.db.insert(playerSessions).values([
			// Five minutes on the server: under the board's default sixty-minute floor.
			{
				serverId: w.server.id,
				steamId: ME,
				name: 'Reader',
				joinedAt: at,
				lastSeen: new Date(at.getTime() + 300_000),
				leftAt: new Date(at.getTime() + 300_000),
				kills: 1,
				cash: 4321
			},
			{
				serverId: w.server.id,
				steamId: VICTIM,
				name: 'Victim',
				joinedAt: at,
				lastSeen: new Date(at.getTime() + 300_000),
				leftAt: new Date(at.getTime() + 300_000),
				deaths: 1,
				cash: 99
			}
		]);
		await env.db.insert(kills).values({
			ts: new Date(at.getTime() + 60_000),
			serverId: w.server.id,
			eventId: 'review-1',
			instanceId: 'i',
			matchId: 'm',
			eventTime: 60,
			map: 'Test',
			killerSteamId: ME,
			killerName: 'Reader',
			killerFaction: 'A',
			victimSteamId: VICTIM,
			victimName: 'Victim',
			victimFaction: 'B',
			tags: []
		});
	});

	test('/api/health tells a stranger it is up and nothing else', async () => {
		const anon = await get('api/health', null);
		expect(anon.body).toEqual({ ok: true, service: 'warcon' });
		const member = await get('api/health', 'viewer');
		expect(member.body).toEqual({ ok: true, service: 'warcon' });
	});

	test('/api/health gives the worker block to the site owner and to METRICS_TOKEN only', async () => {
		const owner = (await get('api/health', 'site')).body as Record<string, unknown>;
		expect(owner.worker).toMatchObject({ servers: 412 });
		env.METRICS_TOKEN = 'a-metrics-token-for-this-test';
		try {
			const wrong = await get('api/health', null, '', { authorization: 'Bearer nope' });
			expect(wrong.body).toEqual({ ok: true, service: 'warcon' });
			const right = await get('api/health', null, '', {
				authorization: 'Bearer a-metrics-token-for-this-test'
			});
			expect((right.body as Record<string, unknown>).worker).toMatchObject({ servers: 412 });
		} finally {
			env.METRICS_TOKEN = undefined;
		}
	});

	test('by design: a public leaderboard row carries the SteamID (the career link) and the in-game cash', async () => {
		const board = (await get('api/public/servers/[id]/leaderboard', null, 'minMinutes=0')).body as {
			rows: Record<string, unknown>[];
		};
		const row = board.rows.find((r) => r.name === 'Reader')!;
		expect(row).toMatchObject({ steamId: ME, cash: 4321 });
	});

	test("by design: the public career links the player's victims and nemeses by SteamID", async () => {
		const { combatSummary } = await import('$lib/server/players');
		const combat = await combatSummary(env, [w.server.id], ME);
		expect(JSON.stringify(combat)).toContain(VICTIM);
		// and the public page hands that summary to the browser as it is
		const page = read(
			'src',
			'routes',
			'(public)',
			's',
			'[id]',
			'players',
			'[steamId]',
			'+page.server.ts'
		);
		expect(page).toContain('combatSummary(env, ids, steamId)');
	});

	test('a TOTP secret and the backup codes are stored encrypted, not as issued', async () => {
		const { initAuth } = await import('$lib/server/auth');
		const auth = initAuth(env);
		const password = 'a-long-test-password';
		const signUp = await auth.api.signUpEmail({
			body: { email: 'totp@warcon.invalid', password, name: 'totp', username: 'totpuser' } as never,
			returnHeaders: true
		});
		const cookie = signUp.headers
			.getSetCookie()
			.map((c) => c.split(';')[0])
			.join('; ');
		const enabled = (await auth.api.enableTwoFactor({
			body: { password },
			headers: new Headers({ cookie })
		})) as { totpURI: string; backupCodes: string[] };
		const issued = new URL(enabled.totpURI).searchParams.get('secret')!;
		expect(issued.length).toBeGreaterThan(10);
		expect(enabled.backupCodes.length).toBeGreaterThan(0);

		const [row] = await env.db.select().from(twoFactor);
		expect(row).toBeDefined();
		expect(row.secret).not.toContain(issued);
		for (const code of enabled.backupCodes) expect(row.backupCodes).not.toContain(code);
		// Neither is the base32 or JSON text it would be if stored plainly.
		expect(() => JSON.parse(row.backupCodes)).toThrow();
	});
});
