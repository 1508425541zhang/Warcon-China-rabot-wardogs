// A watched player joining is posted to the webhooks of the organisation that watches them, and
// to no one else: another org's webhooks, a webhook not ticked for it, or one limited to another
// server hear nothing, and nobody learns of a mark another org keeps.
import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { Env } from '$lib/server/env';
import { playerMarks, webhooks } from '$lib/server/db/schema';
import { encryptSecret } from '$lib/server/crypto';
import { newId } from '$lib/server/http';
import { notifyWatchedJoins, resetWebhookQueues, watchedAmong } from '$lib/server/webhook-delivery';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

const OURS = '76561198000000301';
const THEIRS = '76561198000000302';
const NOBODY = '76561198000000303';

describe.skipIf(!hasTestDb)('watched players joining', () => {
	let env: Env;
	let w: World;
	const posts: { url: string; body: string }[] = [];
	const realFetch = globalThis.fetch;

	const hook = async (
		orgId: string,
		name: string,
		events: string[],
		serverIds: string[] | null = null
	) =>
		env.db.insert(webhooks).values({
			id: newId(),
			orgId,
			label: name,
			urlEnc: encryptSecret(env, `https://discord.test/api/webhooks/1/${name}`),
			events,
			serverIds
		});

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		await env.db.insert(playerMarks).values([
			{ orgId: w.org.id, steamId: OURS, watched: true, reason: 'our staff note' },
			{ orgId: w.otherOrg.id, steamId: THEIRS, watched: true, reason: 'their staff note' },
			// unwatched again: a mark row that no longer watches
			{ orgId: w.org.id, steamId: NOBODY, watched: false, reason: 'old' }
		]);
		await hook(w.org.id, 'wanted', ['watched']);
		await hook(w.org.id, 'bansonly', ['bans']);
		await hook(w.org.id, 'elsewhere', ['watched'], [w.otherServer.id]);
		await hook(w.otherOrg.id, 'theirs', ['watched']);
		globalThis.fetch = (async (url: string | URL, init?: RequestInit) => {
			posts.push({ url: String(url), body: String(init?.body ?? '') });
			return new Response('{"id":"1"}', { status: 200 });
		}) as typeof fetch;
	});

	afterAll(() => {
		globalThis.fetch = realFetch;
		resetWebhookQueues();
	});

	test("the org's own watchlist only", async () => {
		const got = await watchedAmong(env, w.org.id, [
			{ steamId: OURS, name: 'Krieger' },
			{ steamId: THEIRS, name: 'Vasquez' },
			{ steamId: NOBODY, name: 'nightowl' }
		]);
		expect(got).toEqual([{ steamId: OURS, name: 'Krieger', reason: 'our staff note' }]);
	});

	test('posted to the webhook that wants it, and to no other', async () => {
		posts.length = 0;
		await notifyWatchedJoins(env, w.server.id, 'TLR #1', [
			{ steamId: OURS, name: 'Krieger' },
			{ steamId: THEIRS, name: 'Vasquez' },
			{ steamId: NOBODY, name: 'nightowl' }
		]);
		await new Promise((r) => setTimeout(r, 2000));
		expect(posts.map((p) => p.url.split('/').pop()!.split('?')[0])).toEqual(['wanted']);
		const body = posts[0].body;
		expect(body).toContain('Watched player joined');
		expect(body).toContain(OURS);
		expect(body).toContain('our staff note');
		expect(body).not.toContain(THEIRS);
		expect(body).not.toContain('their staff note');
		expect(body).not.toContain('nightowl');
	});

	test("a join on another org's server tells neither org of the first org's mark", async () => {
		posts.length = 0;
		await notifyWatchedJoins(env, w.otherOrgServer.id, 'Theirs', [
			{ steamId: OURS, name: 'Krieger' }
		]);
		await new Promise((r) => setTimeout(r, 2000));
		expect(posts).toEqual([]);
	});
});
