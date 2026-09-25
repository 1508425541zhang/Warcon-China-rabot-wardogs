import { and, eq, sql } from 'drizzle-orm';
import type { Env } from '../env';
import {
	integrityBaselines,
	integrityModelState,
	integrityPlayerMetricHistory
} from '../db/schema';
import type { KillView } from '$lib/types';
import { InfantryWindows } from './windows';
import { generateBatchFeatures, type IntegrityFeatureVector } from './features';
import { weaponOverrides } from './weapon-map';
import { DEFAULT_INTEGRITY_RULES } from './score';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';
import { populationBucket, type MetricCode } from './statistics';

const WINDOW_DAYS = 30;
const MAX_SOURCE_EVENTS = 500_000;
const MAX_PLAYER_DAY_CONTEXT = 20;
const MAX_PLAYER_COHORT = 100;
type Source = 'local' | 'external';
export interface ReplayRow {
	source: Source;
	server_id: string;
	event_id: string;
	at: Date | string;
	instance_id: string;
	match_id: string;
	match_row: number | null;
	event_time: number;
	map: string;
	killer_steam_id: string;
	victim_steam_id: string;
	killer_faction: string;
	victim_faction: string;
	cause: string;
	distance_m: number | null;
	headshot: boolean;
	penetration: boolean;
	player_count: number | null;
}
interface ReferenceSample {
	player: string;
	day: string;
	source: Source;
	serverId: string;
	roundId: string;
	at: Date;
	map: string;
	bucket: string | null;
	metric: MetricCode;
	weapon: string;
	value: number;
	eventId: string;
}
const rank = (id: string): number => {
	let hash = 2166136261;
	for (let i = 0; i < id.length; i++) hash = Math.imul(hash ^ id.charCodeAt(i), 16777619);
	return hash >>> 0;
};
const atDate = (value: Date | string) => (value instanceof Date ? value : new Date(value));

function metricSamples(feature: IntegrityFeatureVector, row: ReplayRow): ReferenceSample[] {
	const day = atDate(row.at).toISOString().slice(0, 10);
	const base = {
		player: row.killer_steam_id,
		day,
		source: row.source,
		serverId: row.server_id,
		roundId: feature.roundId,
		at: atDate(row.at),
		map: feature.map,
		bucket: populationBucket(row.player_count),
		eventId: row.event_id
	};
	const out: ReferenceSample[] = [];
	const add = (metric: MetricCode, value: number | null, weapon = 'INFANTRY') => {
		if (value !== null && Number.isFinite(value)) out.push({ ...base, metric, value, weapon });
	};
	add('kpm180', feature.kpm180);
	add('uniqueVictims', feature.uniqueVictims180);
	add('maxKills15s', feature.maxKills15s);
	add('medianKillInterval', feature.medianKillInterval);
	add('headshotRate', feature.headshotRate);
	add('penetrationRate', feature.penetrationRate);
	for (const weapon of feature.weaponMetrics) {
		if (weapon.kills >= 10)
			add('headshotRateWeapon', weapon.headshots / weapon.kills, weapon.cause);
		if (weapon.kills >= 3) add('maxKillDistanceWeapon', weapon.maxKillDistanceM, weapon.cause);
	}
	return out;
}

function balancedSamples(rows: readonly ReferenceSample[]): ReferenceSample[] {
	const dayGroups = new Map<string, ReferenceSample[]>();
	for (const row of rows) {
		const key = JSON.stringify([
			row.source,
			row.player,
			row.day,
			row.map,
			row.bucket,
			row.metric,
			row.weapon
		]);
		const group = dayGroups.get(key) ?? [];
		group.push(row);
		dayGroups.set(key, group);
	}
	const dayCapped = [...dayGroups.values()].flatMap((group) =>
		group.sort((a, b) => rank(a.eventId) - rank(b.eventId)).slice(0, MAX_PLAYER_DAY_CONTEXT)
	);
	const playerGroups = new Map<string, ReferenceSample[]>();
	for (const row of dayCapped) {
		const key = JSON.stringify([
			row.source,
			row.player,
			row.map,
			row.bucket,
			row.metric,
			row.weapon
		]);
		const group = playerGroups.get(key) ?? [];
		group.push(row);
		playerGroups.set(key, group);
	}
	return [...playerGroups.values()].flatMap((group) =>
		group.sort((a, b) => rank(a.eventId) - rank(b.eventId)).slice(0, MAX_PLAYER_COHORT)
	);
}

export function replayReferenceFeatures(
	rows: readonly ReplayRow[],
	overrides: ReadonlyMap<string, import('./weapons').WeaponCategory>
): ReferenceSample[] {
	const windows = new InfantryWindows();
	const samples: ReferenceSample[] = [];
	for (const row of rows) {
		const event: KillView = {
			eventId: row.event_id,
			ts: atDate(row.at).toISOString(),
			instanceId: row.instance_id,
			matchId: row.match_id,
			matchRow: row.match_row,
			map: row.map,
			eventTime: Number(row.event_time),
			killer: { steamId: row.killer_steam_id, name: '', faction: row.killer_faction },
			victim: { steamId: row.victim_steam_id, name: '', faction: row.victim_faction },
			cause: row.cause,
			distanceM: row.distance_m,
			headshot: row.headshot,
			tags: row.penetration ? ['Penetration'] : [],
			suicide: false,
			teamKill: false
		};
		const feature = generateBatchFeatures(
			windows,
			`${row.source}:${row.server_id}`,
			[event],
			overrides,
			DEFAULT_INTEGRITY_RULES
		).features[0];
		if (feature) samples.push(...metricSamples(feature, row));
	}
	return balancedSamples(samples);
}

interface Cohort {
	source: Source;
	level: number;
	map: string | null;
	bucket: string | null;
	metric: MetricCode;
	weapon: string;
	samples: ReferenceSample[];
}
function cohorts(samples: readonly ReferenceSample[]): Cohort[] {
	const groups = new Map<string, Cohort>();
	for (const sample of samples) {
		for (const [level, map, bucket] of sample.bucket === null
			? [[3, null, null] as const]
			: [
					[1, sample.map, sample.bucket] as const,
					[2, null, sample.bucket] as const,
					[3, null, null] as const
				]) {
			const key = JSON.stringify([sample.source, level, map, bucket, sample.metric, sample.weapon]);
			let group = groups.get(key);
			if (!group) {
				group = {
					source: sample.source,
					level,
					map,
					bucket,
					metric: sample.metric,
					weapon: sample.weapon,
					samples: []
				};
				groups.set(key, group);
			}
			group.samples.push(sample);
		}
	}
	return [...groups.values()];
}

function summarize(group: Cohort) {
	const perPlayer = new Map<string, number>();
	for (const sample of group.samples)
		perPlayer.set(sample.player, (perPlayer.get(sample.player) ?? 0) + 1);
	const weighted = group.samples
		.map((sample) => ({ value: sample.value, weight: 1 / perPlayer.get(sample.player)! }))
		.sort((a, b) => a.value - b.value);
	const total = weighted.reduce((sum, item) => sum + item.weight, 0);
	const quantile = (p: number, source = weighted): number => {
		const target = source.reduce((sum, item) => sum + item.weight, 0) * p;
		let cumulative = 0;
		for (const item of source) {
			cumulative += item.weight;
			if (cumulative >= target) return item.value;
		}
		return source.at(-1)?.value ?? 0;
	};
	const median = quantile(0.5);
	const deviations = weighted
		.map((item) => ({ ...item, value: Math.abs(item.value - median) }))
		.sort((a, b) => a.value - b.value);
	const counts = new Map<number, number>();
	for (const item of weighted) counts.set(item.value, (counts.get(item.value) ?? 0) + item.weight);
	const min = weighted[0].value;
	const max = weighted.at(-1)!.value;
	const bins = Array.from({ length: 30 }, (_, i) => ({
		from: min + ((max - min) * i) / 30,
		to: min + ((max - min) * (i + 1)) / 30,
		count: 0
	}));
	for (const item of weighted)
		bins[
			max === min ? 0 : Math.min(29, Math.floor((30 * (item.value - min)) / (max - min)))
		].count += item.weight;
	const uniquePlayerDays = new Set(group.samples.map((sample) => `${sample.player}:${sample.day}`))
		.size;
	const sumSquaredWeights = weighted.reduce((sum, item) => sum + item.weight ** 2, 0);
	return {
		sampleCount: group.samples.length,
		uniquePlayers: perPlayer.size,
		uniquePlayerDays,
		effectiveSampleSize: total ** 2 / sumSquaredWeights,
		median,
		mad: quantile(0.5, deviations),
		p90: quantile(0.9),
		p95: quantile(0.95),
		p99: quantile(0.99),
		p995: quantile(0.995),
		p999: quantile(0.999),
		p9995: quantile(0.9995),
		cdf: [...counts.entries()].sort((a, b) => a[0] - b[0]),
		histogram: bins
	};
}

export async function refreshCleanIntegrityBaselines(env: Env, orgId: string): Promise<number> {
	const started = Date.now();
	const overrides = await weaponOverrides(env, orgId);
	try {
		return await env.db.transaction(async (tx) => {
			const [locked] = await tx.execute(
				sql`SELECT pg_try_advisory_xact_lock(hashtextextended(${'integrity-baseline:' + orgId}, 0)) AS locked`
			);
			if (!locked?.locked) return 0;
			await tx.insert(integrityModelState).values({ orgId }).onConflictDoNothing();
			const [state] = await tx
				.select()
				.from(integrityModelState)
				.where(eq(integrityModelState.orgId, orgId))
				.for('update');
			const now = new Date();
			const since = new Date(now.getTime() - WINDOW_DAYS * 86_400_000);
			const until = new Date(now.getTime() - 10 * 60_000);
			const rows = (await tx.execute(sql`
				SELECT 'local' AS source, k.server_id, k.event_id, k.ts AS at, k.instance_id,
				k.match_id, k.match_row, k.event_time, k.map, k.killer_steam_id,
				k.victim_steam_id, k.killer_faction, k.victim_faction, k.cause,
				k.distance_m, k.headshot, (k.tags ? 'Penetration') AS penetration,
				pop.player_count
				FROM kills k JOIN servers s ON s.id = k.server_id
				JOIN LATERAL (SELECT player_count FROM samples sm WHERE sm.server_id = k.server_id
					AND sm.ok AND sm.ts <= k.ts AND sm.ts > k.ts - interval '2 minutes'
					ORDER BY sm.ts DESC LIMIT 1) pop ON true
				WHERE s.org_id = ${orgId} AND k.ts >= ${since} AND k.ts < ${until}
				AND k.match_row IS NOT NULL
				AND k.killer_steam_id IS NOT NULL AND k.killer_faction IS NOT NULL
				AND k.victim_faction IS NOT NULL AND NOT k.team_kill AND NOT k.suicide
				AND EXISTS (SELECT 1 FROM feed_processing_jobs j WHERE j.server_id = k.server_id
					AND j.consumer = 'integrity' AND j.kill_ts = k.ts AND j.event_ids ? k.event_id AND j.state = 'done'
					AND j.attempts = 1 AND j.done_at <= j.created_at + interval '5 minutes')
				AND NOT EXISTS (SELECT 1 FROM integrity_cases c WHERE c.org_id = ${orgId}
					AND c.steam_id = k.killer_steam_id)
				AND NOT EXISTS (SELECT 1 FROM integrity_actions a WHERE a.org_id = ${orgId}
					AND a.steam_id = k.killer_steam_id AND a.created_at >= ${since})
				UNION ALL
				SELECT 'external' AS source, 'external:' || i.source_server AS server_id,
				i.event_id, i.event_at AS at, i.instance_id, i.match_id, NULL::bigint AS match_row,
				i.event_time, i.map, i.killer_steam_id, i.victim_steam_id,
				i.killer_faction, i.victim_faction, i.cause, i.distance_m, i.headshot,
				i.penetration, i.player_count
				FROM integrity_import_kills i JOIN integrity_import_batches b ON b.id = i.batch_id
				WHERE i.org_id = ${orgId} AND b.status = 'APPROVED'
				AND i.event_at >= ${since} AND i.event_at < ${until}
				AND NOT EXISTS (SELECT 1 FROM integrity_cases c WHERE c.org_id = ${orgId}
					AND c.steam_id = i.killer_steam_id)
				ORDER BY source, server_id, at, event_time, event_id
				LIMIT ${MAX_SOURCE_EVENTS + 1}
			`)) as ReplayRow[];
			if (rows.length > MAX_SOURCE_EVENTS)
				throw new Error('Baseline replay exceeds bounded event limit.');
			const samples = replayReferenceFeatures(rows, overrides);
			// The clean, case-excluded replay also feeds long-term personal references.
			const byEvent = new Map<string, Map<MetricCode, ReferenceSample>>();
			for (const sample of samples.filter((item) => item.source === 'local')) {
				const key = `${sample.serverId}:${sample.eventId}`;
				const metrics = byEvent.get(key) ?? new Map<MetricCode, ReferenceSample>();
				metrics.set(sample.metric, sample);
				byEvent.set(key, metrics);
			}
			const history = [...byEvent.values()].flatMap((metrics) => {
				const kpm = metrics.get('kpm180');
				if (!kpm || !metrics.has('maxKills15s')) return [];
				return [
					{
						orgId,
						steamId: kpm.player,
						serverId: kpm.serverId,
						roundId: kpm.roundId,
						eventId: kpm.eventId,
						observedAt: kpm.at,
						kpm180: kpm.value,
						headshotRate: metrics.get('headshotRate')?.value ?? null,
						maxKills15s: Math.round(metrics.get('maxKills15s')!.value),
						featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
						modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion
					}
				];
			});
			for (let i = 0; i < history.length; i += 1000)
				await tx
					.insert(integrityPlayerMetricHistory)
					.values(history.slice(i, i + 1000))
					.onConflictDoNothing();
			const generation = crypto.randomUUID();
			const newRows = cohorts(samples)
				.filter((group) => group.samples.length >= 30)
				.map((group) => ({
					id: crypto.randomUUID(),
					orgId,
					source: group.source,
					metric: group.metric,
					level: group.level,
					map: group.map,
					populationBucket: group.bucket,
					weaponCategory: group.weapon,
					...summarize(group),
					windowDays: WINDOW_DAYS,
					calculatedAt: now,
					modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
					featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
					weaponMapVersion: state.weaponMapVersion,
					generation
				}));
			if (!newRows.length) throw new Error('No eligible clean baseline cohorts.');
			await tx.delete(integrityBaselines).where(eq(integrityBaselines.orgId, orgId));
			await tx.insert(integrityBaselines).values(newRows);
			await tx
				.update(integrityModelState)
				.set({
					activeBaselineGeneration: generation,
					baselineStatus: 'READY',
					lastRefreshAt: now,
					lastDurationMs: Date.now() - started,
					updatedAt: now
				})
				.where(eq(integrityModelState.orgId, orgId));
			return newRows.length;
		});
	} catch (error) {
		await env.db
			.insert(integrityModelState)
			.values({
				orgId,
				baselineStatus: 'STALE',
				lastFailureAt: new Date(),
				lastDurationMs: Date.now() - started
			})
			.onConflictDoUpdate({
				target: integrityModelState.orgId,
				set: {
					baselineStatus: 'STALE',
					lastFailureAt: new Date(),
					lastDurationMs: Date.now() - started
				}
			});
		throw error;
	}
}
