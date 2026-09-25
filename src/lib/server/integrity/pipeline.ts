import { and, desc, eq, gte, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityReports, integrityScores, integrityWindows } from '../db/schema';
import { withOwnedTransaction } from '../leadership';
import { memoryOf } from '../observe';
import { notifyIntegrityCase, type IntegrityCaseAlert } from '../webhook-delivery';
import { InfantryWindows } from './windows';
import { weaponOverrides } from './weapon-map';
import { getIntegrityRules } from './rules';
import { scoreIntegrity } from './score';
import { freezeFindingEvidence } from './evidence';
import { captureReportEvidence } from './reports';
import { hadRecentHighRiskWindow } from './history';
import { getProfiles, steamEnabled } from '../steam';
import { steamBanSignals } from './steam-signals';
import { enforceIntegrityCase } from './enforcement';
import { publicMessage } from '../http';
import { evidenceIds, independentEvidence, overlapsEvidence } from './independence';
import type { KillView } from '$lib/types';

const infantry = new InfantryWindows();
const infantryTasks = new Map<string, Promise<void>>();

export function queueIntegrityBatch(env: Env, serverId: string, batch: KillView[]): Promise<void> {
	const previous = infantryTasks.get(serverId) ?? Promise.resolve();
	const task = previous
		.catch(() => {})
		.then(() => processIntegrityBatch(env, serverId, batch))
		.catch((err) => {
			infantry.reset(serverId);
			throw err;
		});
	infantryTasks.set(serverId, task);
	void task
		.finally(() => {
			if (infantryTasks.get(serverId) === task) infantryTasks.delete(serverId);
		})
		.catch(() => {});
	return task;
}

/** Test/diagnostic synchronization without holding up legacy automation in production. */
export async function waitIntegrityBatch(serverId: string): Promise<void> {
	await infantryTasks.get(serverId);
}

export async function processIntegrityBatch(
	env: Env,
	serverId: string,
	batch: KillView[]
): Promise<void> {
	try {
		await captureReportEvidence(env, serverId, batch);
	} catch (err) {
		console.warn(`[warcon] report evidence on ${serverId}:`, publicMessage(err));
	}
	const orgId = memoryOf(serverId)?.server.orgId;
	if (!orgId) return;
	const rules = await getIntegrityRules(env, orgId);
	const findings = infantry.observe(
		serverId,
		batch,
		await weaponOverrides(env, orgId),
		rules.config
	);
	if (!findings.length) return;
	// Only the strongest state of each active window in this batch needs a database write.
	const byWindow = new Map<string, (typeof findings)[number]>();
	for (const finding of findings)
		byWindow.set(`${finding.steamId}:${finding.instanceId}:${finding.anchorClock}`, finding);
	const latest = [...byWindow.values()];
	const profiles = steamEnabled(env)
		? await getProfiles(
				env,
				latest.map((finding) => finding.steamId),
				{ skipFriends: true }
			)
		: new Map();
	const alerts: IntegrityCaseAlert[] = [];
	const candidates: Parameters<typeof enforceIntegrityCase>[1][] = [];
	await withOwnedTransaction(env, async (tx) => {
		for (const finding of latest) {
			const now = new Date();
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
					eventIds: integrityWindows.eventIds
				})
				.from(integrityWindows)
				.where(
					and(
						eq(integrityWindows.orgId, orgId),
						eq(integrityWindows.steamId, finding.steamId),
						gte(
							integrityWindows.observedAt,
							new Date(now.getTime() - rules.config.repeatWindowMinutes * 60_000)
						)
					)
				)
				.orderBy(desc(integrityWindows.observedAt));
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
			const recent = recentRows
				.filter((row) => row.id !== windowId && independentEvidence(row.eventIds, finding.eventIds))
				.slice(0, 2);
			if (windowId === null) {
				const [window] = await tx
					.insert(integrityWindows)
					.values({
						orgId,
						serverId,
						steamId: finding.steamId,
						instanceId: finding.instanceId,
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
						clockFrom: finding.clockFrom,
						clockTo: finding.clockTo,
						infantryKills: finding.infantryKills,
						kpm180: finding.kpm180,
						uniqueVictims: finding.uniqueVictims,
						headshots: finding.headshots,
						penetrations: finding.penetrations,
						burstPoints: finding.burstPoints,
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
				previousKpm: recent.map((row) => row.kpm180),
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
			await tx.insert(integrityScores).values({
				windowId,
				orgId,
				serverId,
				steamId: finding.steamId,
				scoredAt: now,
				ruleVersion: rules.version,
				score: score.score,
				level: score.level,
				breakdown: score.breakdown,
				currentBehaviorAnomaly: score.currentBehaviorAnomaly
			});
			if (score.score >= rules.config.koThreshold) {
				const caseId = await freezeFindingEvidence(tx, {
					orgId,
					serverId,
					steamId: finding.steamId,
					finding,
					score,
					signals,
					steamKnown: steam.known,
					ruleVersion: rules.version,
					rulesSnapshot: rules.config,
					createdAt: now
				});
				alerts.push({
					caseId,
					serverId,
					serverName: memoryOf(serverId)?.server.name ?? serverId,
					steamId: finding.steamId,
					map: finding.map,
					score: score.score,
					level: score.level,
					breakdown: score.breakdown,
					infantryKills: finding.infantryKills,
					kpm180: finding.kpm180,
					uniqueVictims: finding.uniqueVictims,
					uniqueReporters: Number(reporters?.count ?? 0),
					createdAt: now
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
			infantry.markPersisted(serverId, finding, windowId);
		}
	});
	for (const candidate of candidates) {
		try {
			await enforceIntegrityCase(env, candidate);
		} catch (err) {
			console.warn(`[warcon] Integrity enforcement on ${serverId}:`, publicMessage(err));
		}
	}
	for (const alert of alerts) await notifyIntegrityCase(env, orgId, alert);
}
