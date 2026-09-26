import { afterAll, beforeAll, beforeEach, describe, expect, test } from 'bun:test';
import { and, eq, isNull } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	feedProcessingJobs,
	integrityActions,
	integrityCases,
	integrityRules,
	integrityScores,
	integrityWindows,
	kills,
	organizations,
	outbox,
	playerSessions,
	serverLive,
	servers,
	triggers
} from '$lib/server/db/schema';
import { ingestBatch } from '$lib/server/feed';
import {
	claimFeedJob,
	feedBacklogUnsafe,
	feedJobDepth,
	processNextFeedJob
} from '$lib/server/feed-processing';
import { resetIntegrityServer } from '$lib/server/integrity/pipeline';
import { DEFAULT_INTEGRITY_RULES } from '$lib/server/integrity/score';
import { invalidateTriggers } from '$lib/server/triggers';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { forgetMemory, memoryFor } from '$lib/server/observe';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('durable feed processing', () => {
	let env: Env;
	let world: World;
	const killer = '76561198000000950';
	const liveKiller = '76561198000000951';
	const staleKiller = '76561198000000952';
	const recoverKiller = '76561198000000953';
	const backlogKiller = '76561198000000954';
	const victim = (i: number) => `7656119800000${String(960 + i).padStart(4, '0')}`;
	const body = (tag: string, shooter = killer, headshot = false) => ({
		serverId: `boot-${tag}`,
		serverName: 'Test',
		events: Array.from({ length: 24 }, (_, i) => ({
			eventId: `${tag}-${i}`,
			type: 'killed',
			eventTime: i * 4,
			matchId: `match-${tag}`,
			mapName: 'Kavkazi',
			killerSteamId: shooter,
			killerName: 'Shooter',
			victimSteamId: victim(i),
			victimName: `Target ${i}`,
			cause: 'Id.Item.AK74M',
			distance: 500,
			contextTags: headshot ? ['Meta.Progression.Context.Player.KillContext.Headshot'] : []
		}))
	});
	const jobOf = async (id: number) =>
		(await env.db.select().from(feedProcessingJobs).where(eq(feedProcessingJobs.id, id)))[0];
	const jobAt = async (ts: string, consumer: 'legacy' | 'integrity') =>
		(
			await env.db
				.select()
				.from(feedProcessingJobs)
				.where(
					and(
						eq(feedProcessingJobs.killTs, new Date(ts)),
						eq(feedProcessingJobs.consumer, consumer)
					)
				)
		)[0];
	const roster = async () =>
		(
			await env.db
				.select()
				.from(playerSessions)
				.where(and(eq(playerSessions.serverId, world.server.id), isNull(playerSessions.leftAt)))
		).map((session) => ({
			steamId: session.steamId,
			name: session.name,
			faction: session.faction,
			kills: 0,
			deaths: 0,
			cash: 0,
			ping: 20
		}));
	const run = async (job: Awaited<ReturnType<typeof jobOf>>) => {
		if (job.consumer === 'integrity') {
			const observedAt = new Date(job.killTs.getTime() + 1000);
			await env.db
				.insert(serverLive)
				.values({
					serverId: job.serverId,
					playersAt: observedAt,
					statusAt: observedAt,
					status: { map: 'Kavkazi' },
					players: job.serverId === world.server.id ? await roster() : []
				})
				.onConflictDoUpdate({
					target: serverLive.serverId,
					set: { playersAt: observedAt, statusAt: observedAt, status: { map: 'Kavkazi' } }
				});
		}
		return processNextFeedJob(env, job.id, job.consumer as 'legacy' | 'integrity');
	};
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const [org] = await env.db
			.select()
			.from(organizations)
			.where(eq(organizations.id, world.org.id));
		memoryFor(server, org);
		const now = new Date();
		await env.db.insert(playerSessions).values([
			{
				serverId: world.server.id,
				steamId: killer,
				name: 'Shooter',
				faction: 'Blue',
				joinedAt: now,
				lastSeen: now
			},
			{
				serverId: world.server.id,
				steamId: liveKiller,
				name: 'Live',
				faction: 'Blue',
				joinedAt: now,
				lastSeen: now
			},
			{
				serverId: world.server.id,
				steamId: staleKiller,
				name: 'Stale',
				faction: 'Blue',
				joinedAt: now,
				lastSeen: now
			},
			{
				serverId: world.server.id,
				steamId: recoverKiller,
				name: 'Recover',
				faction: 'Blue',
				joinedAt: now,
				lastSeen: now
			},
			...Array.from({ length: 24 }, (_, i) => ({
				serverId: world.server.id,
				steamId: victim(i),
				name: `Target ${i}`,
				faction: 'Red',
				joinedAt: now,
				lastSeen: now
			}))
		]);
		expect(await acquireOrRenew(env, 'feed processing test')).toBe(true);
	});
	afterAll(async () => {
		resetIntegrityServer(world.server.id);
		forgetMemory(world.server.id);
		await releaseOwnership(env);
	});
	beforeEach(async () => {
		const now = new Date();
		await env.db
			.insert(serverLive)
			.values({
				serverId: world.server.id,
				playersAt: now,
				statusAt: now,
				status: { map: 'Kavkazi' },
				players: await roster()
			})
			.onConflictDoUpdate({
				target: serverLive.serverId,
				set: { playersAt: now, statusAt: now, status: { map: 'Kavkazi' }, players: await roster() }
			});
	});

	test('ingest commits kills and a pending job; duplicate batch adds neither', async () => {
		const result = await ingestBatch(env, world.server.id, body('durable'));
		expect(result.accepted).toBe(24);
		const jobs = await env.db
			.select()
			.from(feedProcessingJobs)
			.where(eq(feedProcessingJobs.serverId, world.server.id));
		expect(jobs).toHaveLength(2);
		expect(jobs.map((job) => job.consumer).sort()).toEqual(['integrity', 'legacy']);
		expect(
			jobs.every((job) => job.state === 'pending' && (job.eventIds as string[]).length === 24)
		).toBe(true);
		const duplicate = await ingestBatch(env, world.server.id, body('durable'));
		expect(duplicate.accepted).toBe(0);
		expect(
			await env.db
				.select()
				.from(feedProcessingJobs)
				.where(eq(feedProcessingJobs.serverId, world.server.id))
		).toHaveLength(2);
	});

	test('worker resume consumes persisted events into an Integrity finding', async () => {
		const [job] = await env.db
			.select()
			.from(feedProcessingJobs)
			.where(
				and(
					eq(feedProcessingJobs.serverId, world.server.id),
					eq(feedProcessingJobs.consumer, 'integrity')
				)
			);
		expect(await run(job)).toBe(true);
		expect(
			await run(
				await jobAt(
					(
						await env.db.select().from(feedProcessingJobs).where(eq(feedProcessingJobs.id, job.id))
					)[0].killTs.toISOString(),
					'legacy'
				)
			)
		).toBe(true);
		expect((await jobOf(job.id)).state).toBe('done');
		expect(
			await env.db.select().from(integrityWindows).where(eq(integrityWindows.steamId, killer))
		).toHaveLength(1);
		expect(
			(await env.db.select().from(integrityScores).where(eq(integrityScores.steamId, killer)))
				.length
		).toBeGreaterThan(0);
	});

	test('expired processing lease is reclaimed after a simulated crash', async () => {
		const result = await ingestBatch(env, world.server.id, body('crash'));
		expect(result.accepted).toBe(24);
		const job = await jobAt(result.kills[0].ts, 'legacy');
		const claimed = await claimFeedJob(env, job.id);
		expect(claimed?.state).toBe('processing');
		await env.db
			.update(feedProcessingJobs)
			.set({ leaseUntil: new Date(Date.now() - 1000) })
			.where(eq(feedProcessingJobs.id, job.id));
		expect(await run(job)).toBe(true);
		expect((await jobOf(job.id)).state).toBe('done');
		expect((await jobOf(job.id)).attempts).toBe(2);
		expect(await run(await jobAt(result.kills[0].ts, 'integrity'))).toBe(true);
	});

	test('worker cold start without a server memory snapshot still processes evidence', async () => {
		forgetMemory(world.server.id);
		resetIntegrityServer(world.server.id);
		const result = await ingestBatch(env, world.server.id, body('cold-start'));
		const job = await jobAt(result.kills[0].ts, 'integrity');
		expect(await run(job)).toBe(true);
		expect(await run(await jobAt(result.kills[0].ts, 'legacy'))).toBe(true);
		expect((await jobOf(job.id)).state).toBe('done');
		expect(
			(
				await env.db
					.select()
					.from(integrityWindows)
					.where(eq(integrityWindows.serverId, world.server.id))
			).length
		).toBeGreaterThan(0);
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const [org] = await env.db
			.select()
			.from(organizations)
			.where(eq(organizations.id, world.org.id));
		memoryFor(server, org);
	});

	test('restart restores prior committed kills before evaluating the next small batch', async () => {
		resetIntegrityServer(world.server.id);
		const all = body('split-window', recoverKiller);
		const first = await ingestBatch(env, world.server.id, {
			...all,
			events: all.events.slice(0, 11)
		});
		const firstJob = await jobAt(first.kills[0].ts, 'integrity');
		expect(await run(firstJob)).toBe(true);
		expect(await run(await jobAt(first.kills[0].ts, 'legacy'))).toBe(true);
		expect(
			await env.db
				.select()
				.from(integrityWindows)
				.where(eq(integrityWindows.steamId, recoverKiller))
		).toHaveLength(0);
		resetIntegrityServer(world.server.id);
		const second = await ingestBatch(
			env,
			world.server.id,
			{ ...all, events: all.events.slice(11, 12) },
			new Date(Date.now() + 1000)
		);
		const secondJob = await jobAt(second.kills[0].ts, 'integrity');
		expect(await run(secondJob)).toBe(true);
		expect(await run(await jobAt(second.kills[0].ts, 'legacy'))).toBe(true);
		expect(
			await env.db
				.select()
				.from(integrityWindows)
				.where(eq(integrityWindows.steamId, recoverKiller))
		).toHaveLength(1);
	});

	test('replaying the same job after worker memory loss adds no score, case, action, or outbox', async () => {
		const [job] = await env.db
			.select()
			.from(feedProcessingJobs)
			.where(
				and(
					eq(feedProcessingJobs.serverId, world.server.id),
					eq(feedProcessingJobs.consumer, 'integrity')
				)
			);
		const before = await Promise.all([
			env.db.select().from(integrityScores).where(eq(integrityScores.steamId, killer)),
			env.db.select().from(integrityCases).where(eq(integrityCases.steamId, killer)),
			env.db.select().from(integrityActions).where(eq(integrityActions.steamId, killer)),
			env.db.select().from(outbox).where(eq(outbox.steamId, killer))
		]);
		resetIntegrityServer(world.server.id);
		await env.db
			.update(feedProcessingJobs)
			.set({ state: 'pending', doneAt: null })
			.where(eq(feedProcessingJobs.id, job.id));
		expect(await run(job)).toBe(true);
		const after = await Promise.all([
			env.db.select().from(integrityScores).where(eq(integrityScores.steamId, killer)),
			env.db.select().from(integrityCases).where(eq(integrityCases.steamId, killer)),
			env.db.select().from(integrityActions).where(eq(integrityActions.steamId, killer)),
			env.db.select().from(outbox).where(eq(outbox.steamId, killer))
		]);
		expect(after.map((rows) => rows.length)).toEqual(before.map((rows) => rows.length));
	});

	test('team-kill penalty is not queued twice when a job is replayed', async () => {
		await env.db.insert(triggers).values({
			id: crypto.randomUUID(),
			serverId: world.server.id,
			orgId: world.org.id,
			kind: 'team_kill',
			name: 'Team kill test',
			enabled: true,
			config: { warnAt: 0, warnMessage: '', kickAt: 1, kickReason: 'Team killing' }
		});
		invalidateTriggers(world.server.id);
		await env.db
			.update(playerSessions)
			.set({ faction: 'Blue' })
			.where(eq(playerSessions.steamId, victim(0)));
		await env.db
			.update(serverLive)
			.set({ players: await roster() })
			.where(eq(serverLive.serverId, world.server.id));
		const posted = {
			serverId: 'boot-teamkill',
			serverName: 'Test',
			events: [
				{
					eventId: 'teamkill-1',
					type: 'killed',
					eventTime: 1,
					matchId: 'm',
					mapName: 'Kavkazi',
					killerSteamId: killer,
					killerName: 'Shooter',
					victimSteamId: victim(0),
					victimName: 'Target',
					cause: 'Id.Item.AK74M'
				}
			]
		};
		const result = await ingestBatch(env, world.server.id, posted);
		expect(result.kills[0].teamKill).toBe(true);
		const job = await jobAt(result.kills[0].ts, 'legacy');
		expect(await run(job)).toBe(true);
		const before = await env.db.select().from(outbox).where(eq(outbox.steamId, killer));
		expect(before).toHaveLength(1);
		resetIntegrityServer(world.server.id);
		await env.db
			.update(feedProcessingJobs)
			.set({ state: 'pending', doneAt: null })
			.where(eq(feedProcessingJobs.id, job.id));
		expect(await run(job)).toBe(true);
		const after = await env.db.select().from(outbox).where(eq(outbox.steamId, killer));
		expect(after).toHaveLength(1);
		expect(await run(await jobAt(result.kills[0].ts, 'integrity'))).toBe(true);
	});

	test('stale replay preserves evidence but cannot queue automatic punishment', async () => {
		resetIntegrityServer(world.server.id);
		const now = new Date();
		const roster = [
			liveKiller,
			staleKiller,
			...Array.from({ length: 24 }, (_, i) => victim(i))
		].map((steamId) => ({
			steamId,
			name: steamId,
			faction: steamId === liveKiller || steamId === staleKiller ? 'Blue' : 'Red',
			kills: 0,
			deaths: 0,
			cash: 0,
			ping: 20
		}));
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const [org] = await env.db
			.select()
			.from(organizations)
			.where(eq(organizations.id, world.org.id));
		const memory = memoryFor(server, org);
		memory.players = roster;
		memory.playersAt = now.getTime();
		await env.db
			.insert(serverLive)
			.values({
				serverId: world.server.id,
				ok: true,
				players: roster,
				playersAt: now,
				feedAt: now,
				playerCount: roster.length
			})
			.onConflictDoUpdate({
				target: serverLive.serverId,
				set: {
					ok: true,
					players: roster,
					playersAt: now,
					feedAt: now,
					playerCount: roster.length
				}
			});
		await env.db
			.insert(integrityRules)
			.values({
				orgId: world.org.id,
				config: DEFAULT_INTEGRITY_RULES,
				autoKickEnabled: true,
				autoActionMaxPerHour: 100,
				autoActionMaxPercentOnline: 100
			})
			.onConflictDoUpdate({
				target: integrityRules.orgId,
				set: {
					autoKickEnabled: true,
					autoActionMaxPerHour: 100,
					autoActionMaxPercentOnline: 100
				}
			});
		const live = await ingestBatch(env, world.server.id, body('live-control', liveKiller, true));
		const liveJob = await jobAt(live.kills[0].ts, 'integrity');
		expect(await run(liveJob)).toBe(true);
		expect(await run(await jobAt(live.kills[0].ts, 'legacy'))).toBe(true);
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.steamId, liveKiller))
		).toHaveLength(1);
		resetIntegrityServer(world.server.id);
		const result = await ingestBatch(
			env,
			world.server.id,
			body('stale', staleKiller, true),
			new Date(liveJob.killTs.getTime() + 2000)
		);
		const job = await jobAt(result.kills[0].ts, 'integrity');
		await env.db
			.update(feedProcessingJobs)
			.set({ createdAt: new Date(Date.now() - 15 * 60_000) })
			.where(eq(feedProcessingJobs.id, job.id));
		expect(await run(job)).toBe(true);
		expect(await run(await jobAt(result.kills[0].ts, 'legacy'))).toBe(true);
		expect((await jobOf(job.id)).state).toBe('done');
		expect(
			(await env.db.select().from(integrityCases).where(eq(integrityCases.steamId, staleKiller)))
				.length
		).toBeGreaterThan(0);
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.steamId, staleKiller))
		).toHaveLength(0);
	});

	test('old pending feed backlog preserves a fresh finding but closes automatic action', async () => {
		resetIntegrityServer(world.server.id);
		const now = new Date();
		await env.db.insert(playerSessions).values({
			serverId: world.server.id,
			steamId: backlogKiller,
			name: 'Backlog',
			faction: 'Blue',
			joinedAt: now,
			lastSeen: now
		});
		const [live] = await env.db
			.select()
			.from(serverLive)
			.where(eq(serverLive.serverId, world.server.id));
		const memory = memoryFor(
			(await env.db.select().from(servers).where(eq(servers.id, world.server.id)))[0],
			(await env.db.select().from(organizations).where(eq(organizations.id, world.org.id)))[0]
		);
		const roster = [
			...(live.players as typeof memory.players),
			{
				steamId: backlogKiller,
				name: 'Backlog',
				faction: 'Blue',
				kills: 0,
				deaths: 0,
				cash: 0,
				ping: 20
			}
		];
		await env.db
			.update(serverLive)
			.set({ players: roster, playersAt: now, feedAt: now })
			.where(eq(serverLive.serverId, world.server.id));
		memory.players = roster;
		memory.playersAt = now.getTime();
		const backlogWorld = await seedWorld(env);
		const [old] = await env.db
			.insert(feedProcessingJobs)
			.values({
				serverId: backlogWorld.server.id,
				consumer: 'integrity',
				killTs: now,
				eventIds: ['unprocessed-backlog'],
				createdAt: new Date(now.getTime() - 10 * 60_000)
			})
			.returning();
		const result = await ingestBatch(
			env,
			world.server.id,
			body('backlog-control', backlogKiller, true)
		);
		const job = await jobAt(result.kills[0].ts, 'integrity');
		const depth = await feedJobDepth(env);
		expect(depth.pending).toBeGreaterThanOrEqual(2);
		expect(feedBacklogUnsafe(depth)).toBe(true);
		expect(await run(job)).toBe(true);
		expect(
			(await env.db.select().from(integrityCases).where(eq(integrityCases.steamId, backlogKiller)))
				.length
		).toBeGreaterThan(0);
		expect(
			await env.db
				.select()
				.from(integrityActions)
				.where(eq(integrityActions.steamId, backlogKiller))
		).toHaveLength(0);
		await env.db.delete(feedProcessingJobs).where(eq(feedProcessingJobs.id, old.id));
		expect(await run(await jobAt(result.kills[0].ts, 'legacy'))).toBe(true);
	});

	test('an Integrity retry cannot block the legacy consumer for the same server', async () => {
		const isolated = await seedWorld(env);
		await ingestBatch(env, isolated.server.id, body('consumer-isolation'));
		const jobs = await env.db
			.select()
			.from(feedProcessingJobs)
			.where(eq(feedProcessingJobs.serverId, isolated.server.id));
		const integrity = jobs.find((job) => job.consumer === 'integrity')!;
		const legacy = jobs.find((job) => job.consumer === 'legacy')!;
		await env.db
			.update(feedProcessingJobs)
			.set({ eventIds: ['missing-source-event'] })
			.where(eq(feedProcessingJobs.id, integrity.id));
		expect(await run(integrity)).toBe(true);
		expect((await jobOf(integrity.id)).state).toBe('pending');
		expect(await run(legacy)).toBe(true);
		expect((await jobOf(legacy.id)).state).toBe('done');
	});
	test('a faction change between feed receipt and the next player look cannot become infantry evidence', async () => {
		const result = await ingestBatch(env, world.server.id, body('changed-faction'));
		const event = result.kills[1];
		const [live] = await env.db
			.select({ players: serverLive.players })
			.from(serverLive)
			.where(eq(serverLive.serverId, world.server.id));
		await env.db
			.update(serverLive)
			.set({
				players: (live.players as { steamId: string; faction: string | null }[]).map((player) =>
					player.steamId === event.victim.steamId ? { ...player, faction: 'Blue' } : player
				)
			})
			.where(eq(serverLive.serverId, world.server.id));
		expect(await run(await jobAt(event.ts, 'integrity'))).toBe(true);
		const [row] = await env.db
			.select()
			.from(kills)
			.where(and(eq(kills.serverId, world.server.id), eq(kills.eventId, event.eventId)));
		expect(row.factionBracketed).toBe(true);
		expect(row.victimFaction).toBeNull();
	});

	test('the live player list wins over a stale session faction', async () => {
		await env.db
			.update(playerSessions)
			.set({ faction: 'Blue' })
			.where(
				and(eq(playerSessions.serverId, world.server.id), eq(playerSessions.steamId, victim(1)))
			);
		try {
			const batch = body('roster-over-session');
			const result = await ingestBatch(env, world.server.id, {
				...batch,
				events: batch.events.slice(1, 2)
			});
			expect(result.kills[0].victim.faction).toBe('Red');
			expect(await run(await jobAt(result.kills[0].ts, 'integrity'))).toBe(true);
			const [row] = await env.db
				.select()
				.from(kills)
				.where(eq(kills.eventId, result.kills[0].eventId));
			expect(row.factionBracketed).toBe(true);
			expect(row.victimFaction).toBe('Red');
		} finally {
			await env.db
				.update(playerSessions)
				.set({ faction: 'Red' })
				.where(
					and(eq(playerSessions.serverId, world.server.id), eq(playerSessions.steamId, victim(1)))
				);
		}
	});

	test('an old player list cannot be repaired with a later look or session data', async () => {
		await env.db
			.update(serverLive)
			.set({ playersAt: new Date(Date.now() - 20_000) })
			.where(eq(serverLive.serverId, world.server.id));
		const batch = body('stale-roster');
		const result = await ingestBatch(env, world.server.id, {
			...batch,
			events: batch.events.slice(1, 2)
		});
		expect(result.kills[0].victim.faction).toBeNull();
		expect(await run(await jobAt(result.kills[0].ts, 'integrity'))).toBe(true);
		const [row] = await env.db
			.select()
			.from(kills)
			.where(eq(kills.eventId, result.kills[0].eventId));
		expect(row.factionObservedAt).toBeNull();
		expect(row.factionBracketed).toBe(true);
		expect(row.victimFaction).toBeNull();
	});
});
