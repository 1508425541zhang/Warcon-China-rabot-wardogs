import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { and, eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	integrityActions,
	integrityModelState,
	integrityCases,
	integrityRules,
	integrityScores,
	integrityWindows,
	listEntries,
	outbox,
	organizations,
	serverLive,
	servers
} from '$lib/server/db/schema';
import { DEFAULT_INTEGRITY_RULES, type IntegrityScore } from '$lib/server/integrity/score';
import { decideIntegrityAction, decideStatisticalAction } from '$lib/server/integrity/decisions';
import { getIntegrityRules } from '$lib/server/integrity/rules';
import { enforceIntegrityCase, integrityCapHit } from '$lib/server/integrity/enforcement';
import { effectiveActionKinds, recordIntegrityDelivery } from '$lib/server/integrity/actions';
import type { BehaviorFinding } from '$lib/server/integrity/windows';
import type { StatisticalAssessment } from '$lib/server/integrity/statistics';
import { STATISTICAL_MODEL_CONFIG } from '$lib/server/integrity/statistical-config';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { forgetMemory, memoryFor } from '$lib/server/observe';
import { grantEntry, serverListOf } from '$lib/server/lists';
import { kickBanned } from '$lib/server/lists-sync';
import type { PanelBan } from '$lib/server/lists-plan';
import type { WardogsClient } from '$lib/server/rcon';
import { hasTestDb, testEnv } from './db';
import { callApi, stubGateway } from './call';
import { seedWorld, type World } from './world';

const score: IntegrityScore = {
	score: 85,
	level: 'AUTO_QUARANTINE_ELIGIBLE',
	breakdown: [],
	currentBehaviorAnomaly: true
};
const finding = (steamId: string, extreme = false): BehaviorFinding => ({
	steamId,
	instanceId: `i-${steamId}`,
	roundId: `i-${steamId}:derived:1`,
	map: 'Kavkazi',
	anchorClock: 100,
	windowId: null,
	clockFrom: 0,
	clockTo: 100,
	infantryKills: extreme ? 24 : 18,
	kpm180: extreme ? 8 : 6,
	uniqueVictims: 18,
	headshots: extreme ? 20 : 5,
	headshotPct: extreme ? 83 : 28,
	penetrations: 0,
	penetrationPct: 0,
	burstPoints: extreme ? 12 : 10,
	maxKills15s: extreme ? 8 : 6,
	medianKillInterval: 4,
	reasons: extreme ? ['kpm', 'burst', 'headshot'] : ['kpm', 'burst'],
	eventIds: [`e-${steamId}`]
});
const statistical = (level: StatisticalAssessment['level']): StatisticalAssessment => ({
	status: 'READY',
	level,
	tempoPercentile: level === 'NORMAL' ? 0.5 : 0.9998,
	precisionPercentile: level === 'NORMAL' ? 0.5 : 0.999,
	actionTempoPercentile: level === 'NORMAL' ? 0.5 : 0.9998,
	actionPrecisionPercentile: level === 'NORMAL' ? 0.5 : 0.999,
	strongestMetric: { code: 'kpm180', value: 8, percentile: 0.9998 },
	independentEpisodes: 1,
	sampleCount: 5000,
	metrics: [],
	committee: {
		decision: level === 'KICK_CANDIDATE' ? 'KICK_CANDIDATE' : 'NORMAL',
		autoActionBlocked: false
	} as NonNullable<StatisticalAssessment['committee']>
});

describe('Integrity decision gate', () => {
	test('organization hourly and server online percentage caps use separate counts', () => {
		const settings = { autoActionMaxPerHour: 3, autoActionMaxPercentOnline: 10 };
		expect(integrityCapHit(2, 0, 10, settings)).toBeNull();
		expect(integrityCapHit(3, 0, 10, settings)).toBe('org_hourly');
		expect(integrityCapHit(0, 1, 10, settings)).toBe('server_percent');
	});
	const base = {
		score,
		finding: finding('76561198000000801'),
		confidence: 'B' as const,
		feedHealthy: true,
		playerOnline: true,
		onlinePlayers: 20,
		identityReliable: true,
		priorIndependentWindow: false,
		previousActions: [],
		rules: DEFAULT_INTEGRITY_RULES,
		settings: {
			autoKickEnabled: false,
			autoQuarantine24hEnabled: false,
			autoQuarantine7dEnabled: false,
			autoActionMaxPerHour: 10,
			autoActionMaxPercentOnline: 10,
			autoSuspendedAt: null
		}
	};
	test('defaults and missing evidence cannot execute', () => {
		expect(decideIntegrityAction(base)).toBe('OBSERVE');
		expect(
			decideIntegrityAction({
				...base,
				settings: { ...base.settings, autoKickEnabled: true },
				confidence: 'C'
			})
		).toBe('OBSERVE');
		expect(
			decideIntegrityAction({
				...base,
				settings: { ...base.settings, autoKickEnabled: true },
				feedHealthy: false
			})
		).toBe('OBSERVE');
		expect(
			decideIntegrityAction({
				...base,
				settings: { ...base.settings, autoKickEnabled: true },
				score: { ...score, currentBehaviorAnomaly: false }
			})
		).toBe('OBSERVE');
	});
	test('strong independent behavior allows 24h; exceptional combination allows 7d', () => {
		expect(
			decideIntegrityAction({ ...base, settings: { ...base.settings, autoKickEnabled: true } })
		).toBe('KICK');
		expect(
			decideIntegrityAction({
				...base,
				settings: { ...base.settings, autoQuarantine24hEnabled: true }
			})
		).toBe('QUARANTINE_24H');
		expect(
			decideIntegrityAction({
				...base,
				finding: finding('76561198000000801', true),
				settings: { ...base.settings, autoQuarantine7dEnabled: true }
			})
		).toBe('QUARANTINE_7D');
	});
	test('one extreme signal needs a separate earlier window before 24h quarantine', () => {
		const solo = { ...base.finding, reasons: ['burst' as const], kpm180: 2, burstPoints: 12 };
		const settings = { ...base.settings, autoKickEnabled: true, autoQuarantine24hEnabled: true };
		expect(decideIntegrityAction({ ...base, finding: solo, settings })).toBe('KICK');
		expect(
			decideIntegrityAction({ ...base, finding: solo, settings, priorIndependentWindow: true })
		).toBe('QUARANTINE_24H');
	});
	test('statistical escalation requires a fresh candidate and an effective prior action', () => {
		const candidate = statistical('KICK_CANDIDATE');
		const current = {
			...base,
			finding: finding('76561198000000801', true),
			assessment: candidate,
			settings: {
				...base.settings,
				autoKickEnabled: true,
				autoQuarantine24hEnabled: true,
				autoQuarantine7dEnabled: true
			}
		};
		expect(decideStatisticalAction(current)).toBe('KICK');
		expect(
			decideStatisticalAction({
				...current,
				assessment: { ...candidate, independentEpisodes: 2 },
				priorIndependentWindow: true,
				previousActions: ['KICK']
			})
		).toBe('QUARANTINE_24H');
		expect(
			decideStatisticalAction({
				...current,
				assessment: { ...candidate, independentEpisodes: 3 },
				priorIndependentWindow: true,
				previousActions: ['QUARANTINE_24H']
			})
		).toBe('QUARANTINE_7D');
		expect(
			decideStatisticalAction({
				...current,
				assessment: { ...candidate, independentEpisodes: 3 },
				priorIndependentWindow: true,
				previousActions: []
			})
		).toBe('KICK');
		expect(
			decideStatisticalAction({
				...current,
				assessment: {
					...candidate,
					independentEpisodes: 3,
					committee: { ...candidate.committee!, autoActionBlocked: true }
				},
				priorIndependentWindow: true,
				previousActions: ['QUARANTINE_24H']
			})
		).toBe('OBSERVE');
	});
});

describe.skipIf(!hasTestDb)('experimental Integrity actions', () => {
	let env: Env;
	let world: World;
	const sid = (n: number) => `7656119800000${String(n).padStart(4, '0')}`;
	const online = async (steamId: string) => {
		const now = new Date();
		const players = [steamId, ...Array.from({ length: 9 }, (_, i) => sid(900 + i))].map((id) => ({
			steamId: id,
			name: id,
			faction: 'Blue',
			kills: 0,
			deaths: 0,
			cash: 0,
			ping: 20
		}));
		const memory = memoryFor(
			(await env.db.select().from(servers).where(eq(servers.id, world.server.id)))[0],
			(await env.db.select().from(organizations).where(eq(organizations.id, world.org.id)))[0]
		);
		memory.players = players;
		memory.playersAt = now.getTime();
		await env.db
			.insert(serverLive)
			.values({
				serverId: world.server.id,
				ok: true,
				players,
				playersAt: now,
				feedAt: now,
				playerCount: players.length
			})
			.onConflictDoUpdate({
				target: serverLive.serverId,
				set: { ok: true, players, playersAt: now, feedAt: now, playerCount: players.length }
			});
	};
	const candidate = async (steamId: string, extreme = false) => {
		await online(steamId);
		const f = finding(steamId, extreme);
		const now = new Date();
		const [window] = await env.db
			.insert(integrityWindows)
			.values({
				orgId: world.org.id,
				serverId: world.server.id,
				steamId,
				instanceId: f.instanceId,
				map: f.map,
				clockFrom: f.clockFrom,
				clockTo: f.clockTo,
				observedAt: now,
				infantryKills: f.infantryKills,
				kpm180: f.kpm180,
				uniqueVictims: f.uniqueVictims,
				headshots: f.headshots,
				penetrations: f.penetrations,
				burstPoints: f.burstPoints,
				behaviorReasons: f.reasons,
				eventIds: f.eventIds
			})
			.returning();
		f.windowId = window.id;
		await env.db.insert(integrityScores).values({
			windowId: window.id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId,
			scoredAt: now,
			ruleVersion: 1,
			score: score.score,
			level: score.level,
			breakdown: [],
			currentBehaviorAnomaly: true
		});
		const caseId = `CASE-${steamId}`;
		await env.db.insert(integrityCases).values({
			id: caseId,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId,
			createdAt: now,
			confidence: 'B',
			trigger: 'ABNORMAL_INFANTRY_WINDOW',
			ruleVersion: 1,
			riskScore: score.score,
			riskBreakdown: [],
			snapshot: { behaviorReasons: f.reasons }
		});
		return { orgId: world.org.id, serverId: world.server.id, steamId, caseId, finding: f, score };
	};
	const setFlags = async (patch: Record<string, boolean | number>) => {
		await env.db
			.update(integrityRules)
			.set({
				autoKickEnabled: false,
				autoQuarantine24hEnabled: false,
				autoQuarantine7dEnabled: false,
				autoActionMaxPerHour: 100,
				autoActionMaxPercentOnline: 100,
				autoSuspendedAt: null,
				...patch
			})
			.where(eq(integrityRules.orgId, world.org.id));
	};
	beforeAll(async () => {
		env = await testEnv();
		stubGateway();
		world = await seedWorld(env);
		const [[server], [org]] = await Promise.all([
			env.db.select().from(servers).where(eq(servers.id, world.server.id)),
			env.db.select().from(organizations).where(eq(organizations.id, world.org.id))
		]);
		memoryFor(server, org);
		expect(await acquireOrRenew(env, 'integrity enforcement test')).toBe(true);
		await env.db.insert(integrityRules).values({
			orgId: world.org.id,
			config: { ...DEFAULT_INTEGRITY_RULES, minimumOnlineForAutoAction: 1 }
		});
	});
	afterAll(async () => {
		forgetMemory(world.server.id);
		await releaseOwnership(env);
	});
	test('all switches off leaves a high-risk persisted case untouched', async () => {
		const input = await candidate(sid(801));
		expect(await enforceIntegrityCase(env, input)).toBe('OBSERVE');
		expect(
			await env.db
				.select()
				.from(integrityActions)
				.where(eq(integrityActions.steamId, input.steamId))
		).toHaveLength(0);
	});
	test('kick is queued through the outbox', async () => {
		await setFlags({ autoKickEnabled: true });
		const input = await candidate(sid(802));
		expect(await enforceIntegrityCase(env, input)).toBe('KICK');
		const queued = await env.db
			.select()
			.from(outbox)
			.where(and(eq(outbox.steamId, input.steamId), eq(outbox.action, 'kick')));
		expect(queued).toHaveLength(1);
		expect(queued[0].triggerKind).toBe('integrity');
		const [action] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.caseId, input.caseId));
		expect(action.deliveryState).toBe('pending');
		expect(action.effectiveAt).toBeNull();
		expect(effectiveActionKinds([action])).toEqual([]);
	});
	test('Shadow: legacy KICK still executes when the statistical result is NORMAL', async () => {
		await setFlags({ autoKickEnabled: true });
		const input = await candidate(sid(816));
		const shadow = statistical('NORMAL');
		await env.db
			.update(integrityScores)
			.set({ statistical: shadow })
			.where(eq(integrityScores.windowId, input.finding.windowId!));
		await env.db
			.update(integrityCases)
			.set({ statistical: shadow })
			.where(eq(integrityCases.id, input.caseId));
		expect((await getIntegrityRules(env, world.org.id)).assessmentMode).toBe('statistical_shadow');
		expect(await enforceIntegrityCase(env, input)).toBe('KICK');
	});
	test('Shadow: statistical KICK_CANDIDATE cannot execute when legacy is NORMAL', async () => {
		await setFlags({ autoKickEnabled: true });
		const input = await candidate(sid(817), true);
		const legacy: IntegrityScore = {
			score: 10,
			level: 'NORMAL',
			breakdown: [],
			currentBehaviorAnomaly: false
		};
		const shadow = statistical('KICK_CANDIDATE');
		await env.db
			.update(integrityScores)
			.set({ score: 10, level: 'NORMAL', currentBehaviorAnomaly: false, statistical: shadow })
			.where(eq(integrityScores.windowId, input.finding.windowId!));
		await env.db
			.update(integrityCases)
			.set({ riskScore: 10, statistical: shadow })
			.where(eq(integrityCases.id, input.caseId));
		expect(await enforceIntegrityCase(env, { ...input, score: legacy })).toBe('OBSERVE');
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.caseId, input.caseId))
		).toHaveLength(0);
	});
	test('statistical auto kick requires current provenance and queues exactly one action', async () => {
		await setFlags({ autoKickEnabled: true });
		const input = await candidate(sid(818), true);
		const assessment = statistical('KICK_CANDIDATE');
		await env.db
			.update(integrityRules)
			.set({ assessmentMode: 'statistical', version: 2 })
			.where(eq(integrityRules.orgId, world.org.id));
		try {
			await env.db
				.update(integrityScores)
				.set({ ruleVersion: 2, statistical: assessment })
				.where(eq(integrityScores.windowId, input.finding.windowId!));
			await env.db
				.update(integrityCases)
				.set({ ruleVersion: 2, statistical: assessment })
				.where(eq(integrityCases.id, input.caseId));
			expect(await enforceIntegrityCase(env, input)).toBe('OBSERVE');
			expect(
				await env.db
					.select()
					.from(integrityActions)
					.where(eq(integrityActions.caseId, input.caseId))
			).toHaveLength(0);
			assessment.modelVersion = STATISTICAL_MODEL_CONFIG.modelVersion;
			assessment.featureVersion = STATISTICAL_MODEL_CONFIG.featureVersion;
			assessment.weaponMapVersion = 1;
			assessment.baselineGeneration = 'test-active-baseline';
			assessment.committee!.generation = STATISTICAL_MODEL_CONFIG.modelVersion;
			await env.db
				.insert(integrityModelState)
				.values({
					orgId: world.org.id,
					weaponMapVersion: 1,
					activeBaselineGeneration: 'test-active-baseline',
					baselineStatus: 'READY'
				});
			await env.db
				.update(integrityScores)
				.set({ statistical: assessment })
				.where(eq(integrityScores.windowId, input.finding.windowId!));
			await env.db
				.update(integrityCases)
				.set({ statistical: assessment, snapshot: input.finding })
				.where(eq(integrityCases.id, input.caseId));
			expect(await enforceIntegrityCase(env, input)).toBe('KICK');
			await enforceIntegrityCase(env, input);
			expect(
				await env.db
					.select()
					.from(integrityActions)
					.where(eq(integrityActions.caseId, input.caseId))
			).toHaveLength(1);
			expect(
				await env.db.select().from(outbox).where(eq(outbox.steamId, input.steamId))
			).toHaveLength(1);
		} finally {
			await env.db
				.update(integrityRules)
				.set({ assessmentMode: 'statistical_shadow', version: 1 })
				.where(eq(integrityRules.orgId, world.org.id));
		}
	});
	test('failed and unknown kicks remain ineffective; only delivery counts', async () => {
		await setFlags({ autoKickEnabled: true });
		for (const [n, state] of [
			[810, 'failed'],
			[811, 'unknown'],
			[812, 'delivered']
		] as const) {
			const input = await candidate(sid(n));
			expect(await enforceIntegrityCase(env, input)).toBe('KICK');
			const [queued] = await env.db.select().from(outbox).where(eq(outbox.steamId, input.steamId));
			await env.db.transaction((tx) => recordIntegrityDelivery(tx, queued, state));
			const [action] = await env.db
				.select()
				.from(integrityActions)
				.where(eq(integrityActions.caseId, input.caseId));
			expect(action.deliveryState).toBe(state);
			expect(action.effectiveAt instanceof Date).toBe(state === 'delivered');
			expect(effectiveActionKinds([action])).toEqual(state === 'delivered' ? ['KICK'] : []);
		}
	});
	test('24h panel ban blocks reconnect and expires without a game permanent ban', async () => {
		await setFlags({ autoQuarantine24hEnabled: true });
		const input = await candidate(sid(803));
		expect(await enforceIntegrityCase(env, input)).toBe('QUARANTINE_24H');
		const [action] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.steamId, input.steamId));
		expect(action.source).toBe('RULE');
		expect(action.effectiveAt).toBeInstanceOf(Date);
		expect(action.deliveryState).toBe('pending');
		expect(effectiveActionKinds([action])).toEqual(['QUARANTINE_24H']);
		const [queued] = await env.db.select().from(outbox).where(eq(outbox.steamId, input.steamId));
		await env.db.transaction((tx) => recordIntegrityDelivery(tx, queued, 'failed'));
		const [afterFailedKick] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.id, action.id));
		expect(afterFailedKick.deliveryState).toBe('failed');
		expect(afterFailedKick.effectiveAt).toBeInstanceOf(Date);
		expect(effectiveActionKinds([afterFailedKick])).toEqual(['QUARANTINE_24H']);
		const [savedScore] = await env.db
			.select()
			.from(integrityScores)
			.where(eq(integrityScores.windowId, input.finding.windowId!));
		expect(savedScore.level).toBe('AUTO_QUARANTINE_ELIGIBLE');
		expect(action.expiresAt!.getTime() - Date.now()).toBeGreaterThan(23 * 3600_000);
		const list = await serverListOf(env, { id: world.server.id, orgId: world.org.id }, 'ban');
		const bans = new Map<string, PanelBan>([
			[input.steamId, { steamId: input.steamId, listId: list.id }]
		]);
		const sent: string[] = [];
		const client = {
			json: async (method: string, path: string) => {
				sent.push(`${method} ${path}`);
				return {};
			}
		} as unknown as WardogsClient;
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const [org] = await env.db
			.select()
			.from(organizations)
			.where(eq(organizations.id, world.org.id));
		await kickBanned(env, server, org, client, [input.steamId], bans);
		expect(sent).toHaveLength(1);
		await kickBanned(env, server, org, client, [input.steamId], bans);
		expect(sent).toHaveLength(2);
		await env.db
			.update(listEntries)
			.set({ expiresAt: new Date(Date.now() - 1000) })
			.where(eq(listEntries.id, action.listEntryId!));
		await kickBanned(env, server, org, client, [input.steamId], bans);
		expect(sent).toHaveLength(2);
	});
	test('reverted effective action cannot upgrade a later decision', async () => {
		await setFlags({ autoKickEnabled: true });
		const input = await candidate(sid(813));
		expect(await enforceIntegrityCase(env, input)).toBe('KICK');
		const [queued] = await env.db.select().from(outbox).where(eq(outbox.steamId, input.steamId));
		await env.db.transaction((tx) => recordIntegrityDelivery(tx, queued, 'delivered'));
		const [action] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.caseId, input.caseId));
		await env.db
			.update(integrityActions)
			.set({ revertedAt: new Date() })
			.where(eq(integrityActions.id, action.id));
		const [reverted] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.id, action.id));
		expect(effectiveActionKinds([reverted])).toEqual([]);
		expect(
			decideIntegrityAction({
				...{
					score,
					finding: finding(sid(814)),
					confidence: 'B' as const,
					feedHealthy: true,
					playerOnline: true,
					onlinePlayers: 20,
					identityReliable: true,
					priorIndependentWindow: false,
					rules: DEFAULT_INTEGRITY_RULES
				},
				previousActions: effectiveActionKinds([reverted]),
				settings: {
					autoKickEnabled: false,
					autoQuarantine24hEnabled: true,
					autoQuarantine7dEnabled: true,
					autoActionMaxPerHour: 10,
					autoActionMaxPercentOnline: 10,
					autoSuspendedAt: null
				}
			})
		).toBe('QUARANTINE_24H');
	});
	test('exceptional combined behavior permits 7d only when its own switch is on', async () => {
		await setFlags({ autoQuarantine7dEnabled: true });
		const input = await candidate(sid(804), true);
		expect(await enforceIntegrityCase(env, input)).toBe('QUARANTINE_7D');
		const [action] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.steamId, input.steamId));
		expect(action.expiresAt!.getTime() - Date.now()).toBeGreaterThan(167 * 3600_000);
		const list = await serverListOf(env, { id: world.server.id, orgId: world.org.id }, 'ban');
		const bans = new Map<string, PanelBan>([
			[input.steamId, { steamId: input.steamId, listId: list.id }]
		]);
		let kicks = 0;
		const client = {
			json: async () => {
				kicks++;
				return {};
			}
		} as unknown as WardogsClient;
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const [org] = await env.db
			.select()
			.from(organizations)
			.where(eq(organizations.id, world.org.id));
		await kickBanned(env, server, org, client, [input.steamId], bans);
		await kickBanned(env, server, org, client, [input.steamId], bans);
		expect(kicks).toBe(2);
		await env.db
			.update(listEntries)
			.set({ expiresAt: new Date(Date.now() - 1000) })
			.where(eq(listEntries.id, action.listEntryId!));
		await kickBanned(env, server, org, client, [input.steamId], bans);
		expect(kicks).toBe(2);
	});
	test('an existing manual ban takes priority', async () => {
		await setFlags({ autoQuarantine24hEnabled: true });
		const input = await candidate(sid(805));
		const list = await serverListOf(env, { id: world.server.id, orgId: world.org.id }, 'ban');
		const manual = await grantEntry(env, list, {
			steamId: input.steamId,
			reason: 'human review',
			expiresAt: null,
			addedByName: 'administrator'
		});
		expect(await enforceIntegrityCase(env, input)).toBe('OBSERVE');
		const [entry] = await env.db.select().from(listEntries).where(eq(listEntries.id, manual.id));
		expect(entry.expiresAt).toBeNull();
		const actions = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.steamId, input.steamId));
		expect(actions).toHaveLength(0);
	});
	test('a score saved under an older rule version cannot trigger an action', async () => {
		await setFlags({ autoKickEnabled: true });
		const input = await candidate(sid(815));
		await env.db
			.update(integrityRules)
			.set({ version: 2 })
			.where(eq(integrityRules.orgId, world.org.id));
		try {
			expect(await enforceIntegrityCase(env, input)).toBe('OBSERVE');
			expect(
				await env.db
					.select()
					.from(integrityActions)
					.where(eq(integrityActions.caseId, input.caseId))
			).toHaveLength(0);
		} finally {
			await env.db
				.update(integrityRules)
				.set({ version: 1 })
				.where(eq(integrityRules.orgId, world.org.id));
		}
	});
	test('rate cap suspends actions while cases remain saved', async () => {
		await setFlags({ autoKickEnabled: true, autoActionMaxPerHour: 1 });
		const input = await candidate(sid(806));
		expect(await enforceIntegrityCase(env, input)).toBe('OBSERVE');
		const [row] = await env.db
			.select()
			.from(integrityRules)
			.where(eq(integrityRules.orgId, world.org.id));
		expect(row.autoSuspendedAt).toBeInstanceOf(Date);
		const [caseRow] = await env.db
			.select()
			.from(integrityCases)
			.where(eq(integrityCases.id, input.caseId));
		expect(caseRow).toBeDefined();
	});
	test('rule GET is read-only; only owner can confirm and enable automation', async () => {
		const empty = await getIntegrityRules(env, world.otherOrg.id);
		expect(empty.enforcement.autoKickEnabled).toBe(false);
		expect(
			await env.db.select().from(integrityRules).where(eq(integrityRules.orgId, world.otherOrg.id))
		).toHaveLength(0);
		const { PUT } = await import('../routes/api/orgs/[id]/integrity/enforcement/+server');
		const params = { id: world.org.id };
		const values = { autoKickEnabled: true };
		await setFlags({});
		const operator = await callApi(PUT, world.users.operator, {
			method: 'PUT',
			params,
			body: { values }
		});
		expect(operator.status).toBe(403);
		const unconfirmed = await callApi(PUT, world.users.owner, {
			method: 'PUT',
			params,
			body: { values }
		});
		expect(unconfirmed.status).toBe(400);
		const confirmed = await callApi(PUT, world.users.owner, {
			method: 'PUT',
			params,
			body: { values: { ...values, confirmation: 'ENABLE_EXPERIMENTAL_INTEGRITY' } }
		});
		expect(confirmed.status).toBe(200);
	});
});
