import type { KillView } from '$lib/types';
import { InfantryWindows, type BehaviorFinding, type WeaponWindowMetric } from './windows';
import type { WeaponCategory } from './weapons';
import { DEFAULT_INTEGRITY_RULES, type IntegrityRuleConfig } from './score';

export interface IntegrityFeatureVector {
	eventId: string;
	steamId: string;
	roundId: string;
	map: string;
	at: string;
	infantryKills: number;
	kpm180: number;
	uniqueVictims180: number;
	maxKills15s: number;
	medianKillInterval: number | null;
	headshotRate: number | null;
	penetrationRate: number | null;
	weaponMetrics: WeaponWindowMetric[];
	eventIds: string[];
}

export function featureVector(finding: BehaviorFinding, event: KillView): IntegrityFeatureVector {
	return {
		eventId: event.eventId,
		steamId: finding.steamId,
		roundId: finding.roundId,
		map: finding.map,
		at: event.ts,
		infantryKills: finding.infantryKills,
		kpm180: finding.kpm180,
		uniqueVictims180: finding.uniqueVictims,
		maxKills15s: finding.maxKills15s,
		medianKillInterval: finding.medianKillInterval,
		headshotRate: finding.infantryKills >= 10 ? finding.headshots / finding.infantryKills : null,
		penetrationRate:
			finding.infantryKills >= 10 ? finding.penetrations / finding.infantryKills : null,
		weaponMetrics: finding.weaponMetrics ?? [],
		eventIds: finding.eventIds
	};
}

/** One event cadence for live production and offline historical replay. */
export function generateBatchFeatures(
	windows: InfantryWindows,
	serverId: string,
	batch: readonly KillView[],
	overrides: ReadonlyMap<string, WeaponCategory>,
	config: IntegrityRuleConfig = DEFAULT_INTEGRITY_RULES
): {
	features: IntegrityFeatureVector[];
	findings: BehaviorFinding[];
	snapshots: BehaviorFinding[];
} {
	const features: IntegrityFeatureVector[] = [];
	const findings: BehaviorFinding[] = [];
	const snapshots: BehaviorFinding[] = [];
	const oneRound = batch.every(
		(event) =>
			event.instanceId === batch[0]?.instanceId &&
			event.map === batch[0]?.map &&
			event.matchRow === batch[0]?.matchRow
	);
	const ordered = oneRound ? [...batch].sort((a, b) => a.eventTime - b.eventTime) : batch;
	for (const event of ordered) {
		findings.push(...windows.observe(serverId, [event], overrides, config));
		const steamId = event.killer?.steamId;
		if (!steamId) continue;
		const snapshot = windows.snapshots(serverId, [steamId])[0];
		if (snapshot?.eventIds.includes(event.eventId)) {
			features.push(featureVector(snapshot, event));
			snapshots.push(snapshot);
		}
	}
	return { features, findings, snapshots };
}
