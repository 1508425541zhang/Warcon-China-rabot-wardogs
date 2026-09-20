// The org's ban message: what it renders to, who may change it, who may read it, and that it is
// what a banned player is removed with.
import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { getOrg } from '$lib/server/access';
import { listEntries, organizations, servers } from '$lib/server/db/schema';
import { listOf } from '$lib/server/lists';
import { kickBanned } from '$lib/server/lists-sync';
import type { WardogsClient } from '$lib/server/rcon';
import { banUid, renderBanMessage, unknownBanVars } from '$lib/ban-message';
import { hasTestDb, testEnv } from './db';
import { callApi, stubGateway } from './call';
import { seedWorld, type PrincipalName, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes', 'api');
const PLAYER = '76561198000000091';
const TEMPLATE = '{reason} | Expires {expires} | {duration} | {uid}';

const facts = {
	entryId: '7k2f9a1c-0000-4000-8000-000000000000',
	reason: 'Team killing',
	addedByName: 'Hollis',
	addedAt: new Date('2026-09-19T09:12:00Z'),
	expiresAt: new Date('2026-09-26T09:12:00Z')
};

describe('renderBanMessage', () => {
	test('fills in the facts of the ban, in UTC', () => {
		expect(
			renderBanMessage('{reason} | {duration} | {banned} > {expires} | {uid} | {admin}', facts)
		).toBe('Team killing | 7d | 19 Sep 2026 09:12 UTC > 26 Sep 2026 09:12 UTC | B-7K2F9A | Hollis');
	});

	test('a permanent ban never expires', () => {
		expect(renderBanMessage('{duration}, expires {expires}', { ...facts, expiresAt: null })).toBe(
			'Perm, expires never'
		);
	});

	test('the default message is the reason alone, and an empty reason sends nothing', () => {
		expect(renderBanMessage('{reason}', facts)).toBe('Team killing');
		expect(renderBanMessage('', facts)).toBe('Team killing');
		expect(renderBanMessage('{reason}', { ...facts, reason: '' })).toBe('');
	});

	test('a missing reason leaves no separator dangling at the front', () => {
		expect(renderBanMessage('{reason} | Appeal on Discord', { ...facts, reason: '' })).toBe(
			'Appeal on Discord'
		);
	});

	test('the text never runs past the length of a reason', () => {
		expect(
			renderBanMessage('{reason} {reason}', { ...facts, reason: 'x'.repeat(150) }).length
		).toBe(200);
	});

	test('a placeholder it does not know is named', () => {
		expect(unknownBanVars('{reason} {timeLeft} {Admin} {timeleft}')).toEqual(['timeleft']);
	});

	test('the uid is cut from the entry id', () => {
		expect(banUid(facts.entryId)).toBe('B-7K2F9A');
	});
});

describe.skipIf(!hasTestDb)("an org's ban message", () => {
	let env: Env;
	let w: World;

	const patch = async (who: PrincipalName, banMessage: unknown, orgId = w.org.id) => {
		const mod = await import(join(ROUTES, 'orgs/[id]', '+server.ts'));
		return callApi(mod.PATCH, w.users[who], {
			method: 'PATCH',
			params: { id: orgId },
			body: { banMessage }
		});
	};
	const stored = async () =>
		(
			await env.db
				.select({ banMessage: organizations.banMessage })
				.from(organizations)
				.where(eq(organizations.id, w.org.id))
		)[0].banMessage;

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		stubGateway();
	});

	test('only an owner of the org changes it', async () => {
		for (const who of [
			'anon',
			'stranger',
			'outsider',
			'member',
			'viewer',
			'operator',
			'admin',
			'elsewhere',
			'keyView',
			'keyAll',
			'keyElsewhere'
		] as PrincipalName[]) {
			const answer = await patch(who, 'pwned {reason}');
			expect({ who, refused: answer.status >= 400 }).toEqual({ who, refused: true });
			expect(await stored()).toBe('{reason}');
		}
		const answer = await patch('owner', TEMPLATE);
		expect(answer.status).toBe(200);
		expect(await stored()).toBe(TEMPLATE);
	});

	test('a placeholder it does not know is refused, and blank goes back to the reason alone', async () => {
		const bad = await patch('owner', '{reason} {timeLeft}');
		expect(bad).toMatchObject({ status: 400, code: 'unknown_placeholder' });
		expect(await stored()).toBe(TEMPLATE);

		expect((await patch('owner', '   ')).status).toBe(200);
		expect(await stored()).toBe('{reason}');
		await patch('owner', TEMPLATE);
	});

	test('the kick carries the message, the list keeps the reason', async () => {
		const bans = await listOf(env, w.org.id, 'ban');
		const entryId = 'abc123de-0000-4000-8000-000000000000';
		await env.db.insert(listEntries).values({
			id: entryId,
			listId: bans.id,
			steamId: PLAYER,
			reason: 'aimbot',
			addedByName: 'owner',
			addedAt: new Date('2026-09-19T09:12:00Z'),
			expiresAt: new Date('2099-09-20T09:12:00Z')
		});
		const org = (await getOrg(env, w.org.id))!;
		const [server] = await env.db.select().from(servers).where(eq(servers.id, w.server.id));
		const bodies: unknown[] = [];
		const client = {
			json: async (_m: string, _p: string, body?: unknown) => (bodies.push(body), {})
		} as unknown as WardogsClient;
		await kickBanned(
			env,
			server,
			org,
			client,
			[PLAYER],
			new Map([[PLAYER, { steamId: PLAYER, listId: bans.id }]])
		);
		expect(bodies).toEqual([
			{
				reason: `aimbot | Expires 20 Sep 2099 09:12 UTC | ${Math.round((Date.UTC(2099, 8, 20) - Date.UTC(2026, 8, 19)) / 86400_000)}d | B-ABC123`
			}
		]);
		const [row] = await env.db.select().from(listEntries).where(eq(listEntries.id, entryId));
		expect(row.reason).toBe('aimbot');
	});

	test('a server shows the message to those who ban there, not to a viewer', async () => {
		const mod = await import(join(ROUTES, 'servers/[id]/lists/state', '+server.ts'));
		const state = async (who: PrincipalName) =>
			callApi(mod.GET, w.users[who], { params: { id: w.server.id } });
		const seen = async (who: PrincipalName) =>
			((await state(who)).body as { banMessage?: string | null }).banMessage;

		expect(await seen('owner')).toBe(TEMPLATE);
		expect(await seen('viewer')).toBeNull();
		expect(await seen('keyView')).toBeNull();
		for (const who of ['anon', 'stranger', 'elsewhere', 'keyElsewhere'] as PrincipalName[])
			expect({ who, refused: (await state(who)).status >= 400 }).toEqual({ who, refused: true });
	});
});
