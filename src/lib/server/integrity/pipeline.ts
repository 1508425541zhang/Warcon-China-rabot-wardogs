import { and, desc, eq, gte, isNotNull, lt, sql } from 'drizzle-orm';
import type { Env } from '../env';
import {
	integrityActionEligibility,
	integrityActions,
	integrityCases,
	integrityModelState,
	integrityPlayerCareers,
	integrityReports,
	integrityScores,
	integrityWindows,
	kills,
	servers
} from '../db/schema';
import { killView } from '../feed';
import { withOwnedTransaction } from '../leadership';
import { memoryOf } from '../observe';
import { notifyIntegrityCase, type IntegrityCaseAlert } from '../webhook-delivery';
import { InfantryWindows } from './windows';
import { generateBatchFeatures } from './features';
import { weaponOverrides } from './weapon-map';
import { getIntegrityRules } from './rules';
import { scoreIntegrity } from './score';
import { freezeFindingEvidence } from './evidence';
import { captureReportEvidence } from './reports';
import { hadRecentHighRiskWindow } from './history';
import { cachedProfiles } from '../steam';
import { enqueueIntegrityProfileRefresh } from './profile-refresh';
import { steamBanSignals } from './steam-signals';
import { enforceIntegrityCase } from './enforcement';
import { publicMessage } from '../http';
import { evidenceIds, independentEpisode, overlapsEvidence } from './independence';
import { loadBaselines, loadWeaponBaselines, populationAt } from './baselines';
import { assessDistribution, type StatisticalAssessment } from './statistics';
import { STATISTICAL_AUTO_ACTION_ENABLED, STATISTICAL_MODEL_CONFIG } from './statistical-config';
import { assessCommittee } from './committee';
import { loadCleanCareerContext } from './career-context';
import { shouldRetryActionEligibility } from './action-retry';
import type { KillView } from '$lib/types';

const infantry = new InfantryWindows();

/** Test/recovery seam for simulating a worker process restart. */
export function resetIntegrityServer(serverId: string): void {
	infantry.reset(serverId);
}

export async function processIntegrityBatch(
	env: Env,
	serverId: string,
	batch: KillView[],
	allowActions = true
): Promise<void> {
	try {
		await captureReportEvidence(env, serverId, batch);
	} catch (err) {
		console.warn(`[warcon] report evidence on ${serverId}:`, publicMessage(err));
		throw err;
	}
	// The durable feed queue may wake before the first server poll after a worker restart.
	const orgId =
		memoryOf(serverId)?.server.orgId ??
		(
			await env.db
				.select({ orgId: servers.orgId })
				.from(servers)
				.where(eq(servers.id, serverId))
				.limit(1)
		)[0]?.orgId;
	if (!orgId) return;
	const rules = await getIntegrityRules(env, orgId);
	const overrides = await weaponOverrides(env, orgId);
	if (!infantry.hasServer(serverId) && batch.length) {
		const before = new Date(batch[0].ts);
		const prior = await env.db
			.select()
			.from(kills)
			.where(
				and(
					eq(kills.serverId, serverId),
					eq(kills.instanceId, batch[0].instanceId ?? ''),
					eq(kills.matchId, batch[0].matchId ?? ''),
					eq(kills.map, batch[0].map),
					gte(kills.ts, new Date(before.getTime() - 180_000)),
					lt(kills.ts, before)
				)
			)
			.orderBy(desc(kills.ts), desc(kills.eventTime))
			.limit(1001);
		if (prior.length > 1000) allowActions = false;
		// Rebuild rolling context from committed events before processing the new job.
		infantry.observe(
			serverId,
			prior.slice(0, 1000).reverse().map(killView),
			overrides,
			rules.config
		);
	}
	const generated = generateBatchFeatures(infantry, serverId, batch, overrides, rules.config);
	const findings = generated.findings;
	if (rules.assessmentMode !== 'legacy') {
		findings.push(...generated.snapshots);
	}
	if (!findings.length) return;
	// Only the strongest state of each active window in this batch needs a database write.
	const byWindow = new Map<string, (typeof findings)[number]>();
	for (const finding of findings) {
		const key = finding.snapshotOnly
			? `${finding.steamId}:${finding.roundId}:latest-snapshot`
			: `${finding.steamId}:${finding.roundId}:${finding.anchorClock}`;
		const prior = byWindow.get(key);
		byWindow.set(
			key,
			prior && !prior.snapshotOnly && finding.snapshotOnly
				? { ...finding, reasons: prior.reasons, snapshotOnly: false }
				: finding
		);
	}
	const latest = [...byWindow.values()];
	// A feed batch can cross a map or population change. Resolve each finding from its own event.
	type BaselineContext = {
		baselines: Awaited<ReturnType<typeof loadBaselines>>;
		weaponBaselines: Awaited<ReturnType<typeof loadWeaponBaselines>>;
	};
	const context = new Map<string, BaselineContext>();
	const byFinding = new Map<(typeof latest)[number], BaselineContext | null>();
	for (const finding of latest) {
		if (rules.assessmentMode === 'legacy') {
			byFinding.set(finding, null);
			continue;
		}
		const ids = new Set(finding.eventIds);
		const ownEvent = [...batch]
			.reverse()
			.find((kill) => ids.has(kill.eventId) && kill.map === finding.map);
		const bucket = ownEvent ? await populationAt(env, serverId, new Date(ownEvent.ts)) : null;
		const key = `${finding.map}\u0000${bucket ?? ''}`;
		let selected = context.get(key);
		if (!selected) {
			const [baselines, weaponBaselines] = await Promise.all([
				loadBaselines(env, orgId, finding.map, bucket),
				loadWeaponBaselines(env, orgId, finding.map, bucket)
			]);
			selected = { baselines, weaponBaselines };
			context.set(key, selected);
		}
		byFinding.set(finding, selected);
	}
	// Steam network refresh is never on the ordered feed-consumer path.
	const profiles = await cachedProfiles(
		env,
		latest.map((finding) => finding.steamId)
	);
	const refreshSteam = new Set<string>();
	const alerts: IntegrityCaseAlert[] = [];
	const candidates: Parameters<typeof enforceIntegrityCase>[1][] = [];
	await withOwnedTransaction(env, async (tx) => {
		for (const finding of latest) {
			const now = new Date();
			const selected = byFinding.get(finding);
			const steam = steamBanSignals(profiles.get(finding.steamId), now);
			const [reporters] = await tx
				.select({ count: sql<number>`COUNT(DISTINCT ${integrityReports.reporterSteamId})` })
				.from(integrityReports)
				.where(
					and(
						eq(integrityReports.orgId, orgId),
						eq(integrityReports.targetSteamId, finding.steamId),
						gte(integrityReports.createdAt, new Date(now.getTime() - 24 * 60 * 60_000))
					)
				);
			const recentRows = await tx
				.select({
					id: integrityWindows.id,
					kpm180: integrityWindows.kpm180,
					roundId: integrityWindows.roundId,
					clockTo: integrityWindows.clockTo,
					eventIds: integrityWindows.eventIds,
					observedAt: integrityWindows.observedAt
				})
				.from(integrityWindows)
				.where(
					and(
						eq(integrityWindows.orgId, orgId),
						eq(integrityWindows.steamId, finding.steamId),
						gte(
							integrityWindows.observedAt,
							new Date(
								now.getTime() -
									Math.max(
										rules.config.repeatWindowMinutes * 60_000,
										rules.assessmentMode === 'legacy'
											? 0
											: STATISTICAL_MODEL_CONFIG.persistenceEpisodeHorizonHours * 3_600_000
									)
							)
						)
					)
				)
				.orderBy(desc(integrityWindows.observedAt))
				.limit(500);
			let windowId = finding.windowId;
			// A restarted worker has no in-memory window ID. Recover the saved episode by
			// evidence overlap before allowing any prior window to count as independent.
			if (windowId === null) {
				const saved = await tx
					.select({ id: integrityWindows.id, eventIds: integrityWindows.eventIds })
					.from(integrityWindows)
					.where(
						and(
							eq(integrityWindows.orgId, orgId),
							eq(integrityWindows.serverId, serverId),
							eq(integrityWindows.steamId, finding.steamId),
							eq(integrityWindows.instanceId, finding.instanceId),
							eq(integrityWindows.map, finding.map)
						)
					)
					.orderBy(desc(integrityWindows.observedAt));
				windowId =
					saved.find((row) => overlapsEvidence(row.eventIds, finding.eventIds))?.id ?? null;
			}
			if (windowId !== null) {
				const saved = await tx
					.select({ eventIds: integrityWindows.eventIds })
					.from(integrityWindows)
					.where(eq(integrityWindows.id, windowId))
					.limit(1);
				const known = evidenceIds(saved[0]?.eventIds);
				// A replay of already committed evidence cannot create another score or case.
				if (known && finding.eventIds.every((id) => known.includes(id))) {
					infantry.markPersisted(serverId, finding, windowId);
					continue;
				}
			}
			const independent = recentRows.filter(
				(row) =>
					row.id !== windowId &&
					independentEpisode(
						row,
						{
							eventIds: finding.eventIds,
							roundId: finding.roundId,
							clockFrom: finding.clockFrom,
							observedAt: now
						},
						STATISTICAL_MODEL_CONFIG.minimumEpisodeSeparationSeconds
					)
			);
			const recentLegacy = independent
				.filter(
					(row) =>
						row.observedAt.getTime() >= now.getTime() - rules.config.repeatWindowMinutes * 60_000
				)
				.slice(0, 2);
			const recentStatistical = independent
				.filter(
					(row) =>
						row.observedAt.getTime() >=
						now.getTime() - STATISTICAL_MODEL_CONFIG.persistenceEpisodeHorizonHours * 3_600_000
				)
				.slice(0, 2);
			const statistical: StatisticalAssessment | null = selected
				? assessDistribution(
						{
							kpm180: finding.kpm180,
							uniqueVictims: finding.uniqueVictims,
							maxKills15s: finding.maxKills15s,
							medianKillInterval: finding.medianKillInterval,
							headshotRate: finding.headshots / finding.infantryKills,
							penetrationRate: finding.penetrations / finding.infantryKills
						},
						selected.baselines,
						finding.infantryKills,
						recentStatistical.length + 1,
						finding.weaponMetrics ?? [],
						selected.weaponBaselines
					)
				: null;
			if (statistical) {
				const currentEvent = [...batch]
					.reverse()
					.find((event) => finding.eventIds.includes(event.eventId));
				const career =
					statistical.status === 'READY'
						? await loadCleanCareerContext(
								tx,
								orgId,
								finding.steamId,
								currentEvent ? new Date(currentEvent.ts) : now
							)
						: null;
				const recentKpm = [...recentStatistical]
					.reverse()
					.map((row) => row.kpm180)
					.concat(finding.kpm180);
				statistical.modelVersion = STATISTICAL_MODEL_CONFIG.modelVersion;
				statistical.featureVersion = STATISTICAL_MODEL_CONFIG.featureVersion;
				const generations = new Set(statistical.metrics.map((metric) => metric.baselineGeneration));
				const mappings = new Set(statistical.metrics.map((metric) => metric.weaponMapVersion));
				statistical.weaponMapVersion = mappings.size === 1 ? ([...mappings][0] ?? null) : null;
				statistical.baselineGeneration =
					generations.size === 1 ? ([...generations][0] ?? null) : null;
				statistical.baselineCalculatedAt = statistical.metrics.length
					? statistical.metrics.map((metric) => metric.calculatedAt).sort()[0]
					: null;
				const localActionMetrics = statistical.metrics.filter(
					(metric) => metric.source === 'local' && metric.code !== 'maxKillDistanceWeapon'
				);
				const nowMs = now.getTime();
				statistical.committee = assessCommittee(
					{
						statistical,
						independentEpisodes: recentStatistical.length + 1,
						career: career
							? {
									sampleCount: career.sampleCount,
									uniqueDays: career.uniqueDays,
									kpmMedian: career.kpmMedian,
									kpmMad: career.kpmMad,
									recentKpm
								}
							: undefined,
						changeSeries: career ? [...career.orderedKpm, ...recentKpm] : undefined,
						currentKpm: finding.kpm180,
						eventIds: finding.eventIds
					},
					{
						feedHealthy: !!memoryOf(serverId) && nowMs - memoryOf(serverId)!.playersAt < 5 * 60_000,
						backlogSafe: allowActions,
						identityReliable: !!finding.steamId,
						roundReliable: finding.roundId.includes(':match:'),
						baselineFresh:
							localActionMetrics.length > 0 &&
							localActionMetrics.every(
								(metric) =>
									nowMs - Date.parse(metric.calculatedAt) <
									STATISTICAL_MODEL_CONFIG.maximumBaselineAgeHours * 3_600_000
							),
						versionsMatch:
							!!statistical.baselineGeneration &&
							statistical.weaponMapVersion !== null &&
							statistical.metrics.every(
								(metric) =>
									metric.modelVersion === STATISTICAL_MODEL_CONFIG.modelVersion &&
									metric.featureVersion === STATISTICAL_MODEL_CONFIG.featureVersion &&
									metric.weaponMapVersion === statistical.weaponMapVersion &&
									metric.baselineGeneration === statistical.baselineGeneration
							),
						baselinePopulationAdequate:
							localActionMetrics.length > 0 &&
							localActionMetrics.every(
								(metric) =>
									metric.sampleCount >= STATISTICAL_MODEL_CONFIG.minimumBaselineSamples &&
									(metric.uniquePlayers ?? 0) >= STATISTICAL_MODEL_CONFIG.minimumBaselinePlayers &&
									(metric.uniquePlayerDays ?? 0) >=
										STATISTICAL_MODEL_CONFIG.minimumBaselinePlayerDays &&
									(metric.effectiveSampleSize ?? 0) >=
										STATISTICAL_MODEL_CONFIG.minimumEffectiveSampleSize
							)
					}
				);
				statistical.level =
					statistical.committee.decision === 'KICK_CANDIDATE'
						? 'KICK_CANDIDATE'
						: statistical.committee.decision === 'WATCH'
							? 'WATCH'
							: 'NORMAL';
			}
			if (
				finding.reasons.length ||
				statistical?.level === 'CASE' ||
				statistical?.level === 'KICK_CANDIDATE'
			)
				refreshSteam.add(finding.steamId);
			if (
				finding.snapshotOnly &&
				(!statistical ||
					statistical.status !== 'READY' ||
					statistical.committee?.decision === 'NORMAL')
			)
				continue;
			if (
				!finding.reasons.length &&
				(!statistical ||
					statistical.level === 'NORMAL' ||
					statistical.status === 'INSUFFICIENT_DATA')
			)
				continue;
			const [previousAssessment] =
				windowId === null
					? []
					: await tx
							.select({ statistical: integrityScores.statistical })
							.from(integrityScores)
							.where(eq(integrityScores.windowId, windowId))
							.orderBy(desc(integrityScores.id))
							.limit(1);
			if (windowId === null) {
				const [window] = await tx
					.insert(integrityWindows)
					.values({
						orgId,
						serverId,
						steamId: finding.steamId,
						instanceId: finding.instanceId,
						roundId: finding.roundId,
						map: finding.map,
						clockFrom: finding.clockFrom,
						clockTo: finding.clockTo,
						observedAt: now,
						infantryKills: finding.infantryKills,
						kpm180: finding.kpm180,
						uniqueVictims: finding.uniqueVictims,
						headshots: finding.headshots,
						penetrations: finding.penetrations,
						burstPoints: finding.burstPoints,
						maxKills15s: finding.maxKills15s,
						medianKillInterval: finding.medianKillInterval,
						behaviorReasons: finding.reasons,
						eventIds: finding.eventIds
					})
					.returning({ id: integrityWindows.id });
				windowId = window.id;
			} else {
				const [saved] = await tx
					.select({ eventIds: integrityWindows.eventIds })
					.from(integrityWindows)
					.where(eq(integrityWindows.id, windowId));
				if (!saved) throw new Error('Integrity episode disappeared before update.');
				await tx
					.update(integrityWindows)
					.set({
						roundId: finding.roundId,
						clockFrom: finding.clockFrom,
						clockTo: finding.clockTo,
						infantryKills: finding.infantryKills,
						kpm180: finding.kpm180,
						uniqueVictims: finding.uniqueVictims,
						headshots: finding.headshots,
						penetrations: finding.penetrations,
						burstPoints: finding.burstPoints,
						maxKills15s: finding.maxKills15s,
						medianKillInterval: finding.medianKillInterval,
						behaviorReasons: finding.reasons,
						eventIds: [...new Set([...(evidenceIds(saved.eventIds) ?? []), ...finding.eventIds])]
					})
					.where(eq(integrityWindows.id, windowId));
			}
			const repeatHighRiskWindow = await hadRecentHighRiskWindow(
				tx,
				orgId,
				finding.steamId,
				now,
				rules.config.repeatKoWindowHours,
				windowId,
				finding.eventIds
			);
			const signals = {
				behaviorReasons: finding.reasons,
				kpm180: finding.kpm180,
				uniqueVictims: finding.uniqueVictims,
				previousKpm: recentLegacy.map((row) => row.kpm180),
				uniqueReporters: Number(reporters?.count ?? 0),
				repeatHighRiskWindow,
				infantryKills: finding.infantryKills,
				headshots: finding.headshots,
				penetrations: finding.penetrations,
				burstPoints: finding.burstPoints,
				vacBans: steam.vacBans,
				gameBans: steam.gameBans,
				daysSinceLastBan: steam.daysSinceLastBan,
				wardogsPlaytimeHours: null
			};
			const score = scoreIntegrity(signals, rules.config);
			const [savedScore] = await tx
				.insert(integrityScores)
				.values({
					windowId,
					orgId,
					serverId,
					steamId: finding.steamId,
					scoredAt: now,
					ruleVersion: rules.version,
					score: score.score,
					level: score.level,
					breakdown: score.breakdown,
					statistical,
					currentBehaviorAnomaly: score.currentBehaviorAnomaly
				})
				.returning({ id: integrityScores.id });
			const legacyCase = score.score >= rules.config.koThreshold;
			const statisticalCase =
				statistical?.level === 'CASE' || statistical?.level === 'KICK_CANDIDATE';
			const newStatisticalLevel =
				statisticalCase &&
				(previousAssessment?.statistical as StatisticalAssessment | null)?.level !==
					statistical?.level;
			// Persist the assessment transition once. Eligibility retries reuse that frozen case.
			const priorCases = statisticalCase
				? await tx
						.select({
							id: integrityCases.id,
							status: integrityCases.status,
							snapshot: integrityCases.snapshot,
							statistical: integrityCases.statistical
						})
						.from(integrityCases)
						.where(
							and(
								eq(integrityCases.orgId, orgId),
								eq(integrityCases.serverId, serverId),
								eq(integrityCases.steamId, finding.steamId),
								isNotNull(integrityCases.statistical)
							)
						)
						.orderBy(desc(integrityCases.createdAt))
						.limit(20)
				: [];
			const sameEpisodeCase = priorCases.find((item) => {
				const snapshot = item.snapshot as { roundId?: string; eventIds?: string[] };
				return (
					snapshot.roundId === finding.roundId &&
					Array.isArray(snapshot.eventIds) &&
					snapshot.eventIds.some((id) => finding.eventIds.includes(id)) &&
					(item.statistical as StatisticalAssessment | null)?.level === statistical?.level
				);
			});
			let caseId = sameEpisodeCase?.id ?? null;
			const createCase =
				rules.assessmentMode === 'statistical'
					? statisticalCase &&
						!sameEpisodeCase &&
						(newStatisticalLevel || statistical?.level === 'KICK_CANDIDATE')
					: legacyCase || newStatisticalLevel;
			if (createCase) {
				caseId = await freezeFindingEvidence(tx, {
					orgId,
					serverId,
					steamId: finding.steamId,
					finding,
					score,
					signals,
					steamKnown: steam.known,
					ruleVersion: rules.version,
					rulesSnapshot: rules.config,
					createdAt: now,
					scoreId: savedScore.id,
					statistical,
					trigger:
						!legacyCase && statisticalCase ? 'STATISTICAL_WINDOW' : 'ABNORMAL_INFANTRY_WINDOW'
				});
				await tx
					.update(integrityPlayerCareers)
					.set({ status: 'FROZEN', updatedAt: now })
					.where(
						and(
							eq(integrityPlayerCareers.orgId, orgId),
							eq(integrityPlayerCareers.steamId, finding.steamId)
						)
					);
				await tx
					.update(integrityModelState)
					.set({ baselineStatus: 'STALE', updatedAt: now })
					.where(eq(integrityModelState.orgId, orgId));
				if (legacyCase || (rules.assessmentMode === 'statistical' && statisticalCase))
					alerts.push({
						caseId,
						serverId,
						serverName: memoryOf(serverId)?.server.name ?? serverId,
						steamId: finding.steamId,
						map: finding.map,
						score: score.score,
						level: score.level,
						statisticalLevel: statistical?.level ?? null,
						breakdown: score.breakdown,
						infantryKills: finding.infantryKills,
						kpm180: finding.kpm180,
						uniqueVictims: finding.uniqueVictims,
						uniqueReporters: Number(reporters?.count ?? 0),
						createdAt: now
					});
			}
			if (
				caseId &&
				(rules.assessmentMode === 'statistical'
					? statistical?.level === 'KICK_CANDIDATE' && STATISTICAL_AUTO_ACTION_ENABLED
					: legacyCase && createCase)
			) {
				const [action] = await tx
					.select({ id: integrityActions.id })
					.from(integrityActions)
					.where(eq(integrityActions.caseId, caseId))
					.limit(1);
				const [attempt] = await tx
					.select()
					.from(integrityActionEligibility)
					.where(eq(integrityActionEligibility.caseId, caseId))
					.limit(1);
				if (
					shouldRetryActionEligibility({
						caseOpen: !sameEpisodeCase || sameEpisodeCase.status === 'OPEN',
						hasAction: !!action,
						lastAttemptAt: attempt?.lastAttemptAt ?? null,
						now
					})
				) {
					await tx
						.insert(integrityActionEligibility)
						.values({ caseId, lastAttemptAt: now })
						.onConflictDoUpdate({
							target: integrityActionEligibility.caseId,
							set: { lastAttemptAt: now, attempts: sql`${integrityActionEligibility.attempts} + 1` }
						});
					candidates.push({
						orgId,
						serverId,
						steamId: finding.steamId,
						caseId,
						finding: { ...finding, windowId },
						score
					});
				}
			}
			infantry.markPersisted(serverId, finding, windowId);
		}
	});
	if (refreshSteam.size) await enqueueIntegrityProfileRefresh(env, [...refreshSteam]);
	// Steam lookups and scoring can outlive the live decision window.
	const stillLive = batch.length > 0 && Date.now() - Date.parse(batch[0].ts) <= 5 * 60_000;
	for (const candidate of allowActions && stillLive ? candidates : []) {
		try {
			await enforceIntegrityCase(env, candidate);
		} catch (err) {
			console.warn(`[warcon] Integrity enforcement on ${serverId}:`, publicMessage(err));
		}
	}
	for (const alert of alerts) await notifyIntegrityCase(env, orgId, alert);
}
