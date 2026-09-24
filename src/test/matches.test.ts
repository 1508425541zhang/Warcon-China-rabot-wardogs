// Match history: the list and the page, in the panel (View) and on the public site (the
// leaderboards switch), and what each refuses: another server's match, a match still running.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	kills,
	matches,
	matchPlayers,
	organizations,
	samples,
	servers
} from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { callApi, callLoad, stubGateway } from './call';
import { seedWorld, type PrincipalName, type World } from './world';
import { GET as listRoute } from '../routes/api/servers/[id]/matches/+server';
import { GET as matchRoute } from '../routes/api/servers/[id]/matches/[matchId]/+server';
import { GET as killsRoute } from '../routes/api/servers/[id]/kills/+server';
import { GET as publicList } from '../routes/api/public/servers/[id]/matches/+server';
import { GET as publicMatch } from '../routes/api/public/servers/[id]/matches/[matchId]/+server';
import { load as panelLoad } from '../routes/(app)/server/[id]/matches/[matchId]/+page.server';
import { load as publicPageLoad } from '../routes/(public)/s/[id]/matches/[matchId]/+page.server';
import type { MatchListView, MatchView } from '$lib/matches';

const A = '76561198000000081';
const B = '76561198000000082';
const HOUR = 3_600_000;

describe.skipIf(!hasTestDb)('match history', () => {
	let env: Env;
	let w: World;
	let ended = 0;
	let running = 0;
	let elsewhere = 0;
	const t0 = Date.now() - 6 * HOUR;
	const at = (ms: number) => new Date(t0 + ms);
	const scores = [
		{ name: 'Valkyra', score: 100 },
		{ name: 'Lonestar', score: 64 }
	];

	beforeAll(async () => {
		env = await testEnv();
		stubGateway();
		w = await seedWorld(env);
		const rows = await env.db
			.insert(matches)
			.values([
				{
					serverId: w.server.id,
					startedAt: at(0),
					endedAt: at(HOUR),
					map: 'Kavkazi',
					finalScores: scores,
					winner: 'Valkyra',
					peakPlayers: 2
				},
				{ serverId: w.server.id, startedAt: at(2 * HOUR), map: 'Europe', peakPlayers: 1 },
				{
					serverId: w.otherServer.id,
					startedAt: at(0),
					endedAt: at(HOUR),
					map: 'Europe',
					finalScores: scores,
					winner: 'Valkyra'
				}
			])
			.returning({ id: matches.id });
		[ended, running, elsewhere] = rows.map((r) => r.id);
		await env.db.insert(matchPlayers).values([
			{
				matchId: ended,
				serverId: w.server.id,
				steamId: A,
				name: 'Nomad',
				faction: 'Valkyra',
				seconds: 3600,
				kills: 31,
				deaths: 5,
				cashDelta: 4000,
				headshots: 6,
				killStreak: 9,
				longestM: 120
			},
			{
				matchId: ended,
				serverId: w.server.id,
				steamId: B,
				name: 'Dutchie',
				faction: 'Lonestar',
				seconds: 1800,
				kills: 12,
				deaths: 1,
				cashDelta: 12_400,
				longestM: 412,
				killStreak: 4
			}
		]);
		await env.db.insert(samples).values(
			[0, 20, 40, 60].map((min) => ({
				serverId: w.server.id,
				ts: at(min * 60_000),
				ok: true,
				playerCount: 2,
				map: 'Kavkazi',
				scores: [
					{ name: 'Valkyra', score: Math.min(100, min * 2) },
					{ name: 'Lonestar', score: Math.round(min * 1.1) }
				]
			}))
		);
		await env.db.insert(kills).values(
			[10, 20, 30].map((secs) => ({
				ts: at(secs * 1000),
				serverId: w.server.id,
				eventId: `mh${secs}`,
				instanceId: 'i',
				matchId: 'g',
				matchRow: secs === 30 ? running : ended,
				eventTime: secs,
				map: 'Kavkazi',
				killerSteamId: A,
				killerName: 'Nomad',
				killerFaction: 'Valkyra',
				victimSteamId: B,
				victimName: 'Dutchie',
				victimFaction: 'Lonestar',
				cause: 'Id.Item.AK74M',
				distanceM: 50,
				tags: []
			}))
		);
	});

	const as = (who: PrincipalName) => w.users[who];
	const publicOn = async (on: boolean) => {
		await env.db
			.update(organizations)
			.set({ allowPublicLeaderboards: true })
			.where(eq(organizations.id, w.org.id));
		await env.db.update(servers).set({ publicLeaderboards: on }).where(eq(servers.id, w.server.id));
	};

	test('the list is the matches table alone, newest first, with the match in progress on top, by page', async () => {
		const r = await callApi(listRoute, as('viewer'), { params: { id: w.server.id } });
		expect(r.status).toBe(200);
		const body = r.body as MatchListView;
		expect(body.matches.map((m) => [m.id, m.endedAt === null, m.players, m.winner])).toEqual([
			[running, true, 0, null],
			[ended, false, 2, 'Valkyra']
		]);
		expect([body.page, body.pageSize, body.total, body.pages]).toEqual([1, 50, 2, 1]);
		const beyond = await callApi(listRoute, as('viewer'), {
			params: { id: w.server.id },
			query: 'page=2'
		});
		expect((beyond.body as MatchListView).matches).toEqual([]);
		expect((beyond.body as MatchListView).page).toBe(2);
		const bad = await callApi(listRoute, as('viewer'), {
			params: { id: w.server.id },
			query: 'page=x'
		});
		expect((bad.body as MatchListView).page).toBe(1);
	});

	test('a match page has the lines by kills, the timeline in seconds and the awards', async () => {
		const r = await callApi(matchRoute, as('viewer'), {
			params: { id: w.server.id, matchId: String(ended) }
		});
		expect(r.status).toBe(200);
		const v = r.body as MatchView;
		expect(v.lines.map((l) => [l.steamId, l.kills, l.result])).toEqual([
			[A, 31, 'win'],
			[B, 12, 'loss']
		]);
		expect(v.factions.map((f) => f.name)).toEqual(['Valkyra', 'Lonestar']);
		// the sample at the end belongs to the next match (the boundary look's scores) and is left out
		expect(v.timeline).toEqual([
			[0, 0, 0],
			[1200, 40, 22],
			[2400, 80, 44]
		]);
		expect(v.awards.map((a) => [a.key, a.name])).toEqual([
			['kills', 'Nomad'],
			['kd', 'Dutchie'],
			['longest', 'Dutchie'],
			['streak', 'Nomad'],
			['cash', 'Dutchie']
		]);
		expect(v.kills).toBe(2);
		expect(v.hasFeed).toBe(false);
	});

	test("another server's match, a match still running and a bad id are not found", async () => {
		for (const matchId of [String(elsewhere), String(running), '12x', '0'.repeat(30)]) {
			const r = await callApi(matchRoute, as('owner'), { params: { id: w.server.id, matchId } });
			expect([matchId, r.status]).toEqual([matchId, 404]);
			const load = await callLoad(panelLoad, as('owner'), { params: { id: w.server.id, matchId } });
			expect([matchId, load.status]).toEqual([matchId, 404]);
		}
	});

	test("the kills route narrowed to a match carries that match's kills only", async () => {
		const r = await callApi(killsRoute, as('viewer'), {
			params: { id: w.server.id },
			query: `match=${ended}&count=1`
		});
		expect(r.status).toBe(200);
		const body = r.body as { kills: { eventId: string }[]; total: number };
		expect(body.kills.map((k) => k.eventId).sort()).toEqual(['mh10', 'mh20']);
		expect(body.total).toBe(2);
		const other = await callApi(killsRoute, as('viewer'), {
			params: { id: w.server.id },
			query: `match=${elsewhere}`
		});
		expect(other.status).toBe(404);
	});

	test('the public list and page answer only while leaderboards are public, without SteamIDs in the feed', async () => {
		await publicOn(false);
		expect((await callApi(publicList, null, { params: { id: w.server.id } })).status).toBe(404);
		expect(
			(await callApi(publicMatch, null, { params: { id: w.server.id, matchId: String(ended) } }))
				.status
		).toBe(404);
		expect(
			(
				await callLoad(publicPageLoad, null, {
					params: { id: w.server.id, matchId: String(ended) }
				})
			).status
		).toBe(404);
		await publicOn(true);
		const list = await callApi(publicList, null, { params: { id: w.server.id } });
		expect(list.status).toBe(200);
		expect((list.body as MatchListView).matches.length).toBe(2);
		expect((list.body as MatchListView).pageSize).toBe(20);
		const page = await callApi(publicMatch, null, {
			params: { id: w.server.id, matchId: String(ended) }
		});
		expect(page.status).toBe(200);
		const body = page.body as MatchView & {
			feed: { killer: { name: string; faction: string | null; steamId?: string } | null }[];
		};
		expect(body.lines.length).toBe(2);
		expect(body.feed.length).toBe(2);
		expect(body.feed[0].killer).toEqual({ name: 'Nomad', faction: 'Valkyra' });
		expect(JSON.stringify(body.feed)).not.toContain(A);
		const load = await callLoad(publicPageLoad, null, {
			params: { id: w.server.id, matchId: String(ended) }
		});
		expect(load.status).toBe(200);
		expect(
			(await callApi(publicMatch, null, { params: { id: w.server.id, matchId: String(running) } }))
				.status
		).toBe(404);
	});
});
