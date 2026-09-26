import type { StatisticalAssessment } from './statistics';
import type { CommitteeDecision, ExpertDecision } from './committee';

const decisions: CommitteeDecision[] = [
	'NORMAL',
	'WATCH',
	'CASE',
	'KICK_CANDIDATE',
	'ESCALATION_CANDIDATE'
];
const verdicts: ExpertDecision[] = ['NORMAL', 'SUSPICIOUS', 'CHEAT_LIKELY', 'UNKNOWN'];

/** Counts one latest frozen assessment per persisted episode. Never infer an executed action. */
export function summarizeCommitteeShadow(
	rows: readonly { windowId: number | null; statistical: unknown }[],
	labels: readonly { caseId: string; label: string; statistical: unknown }[],
	truncated: boolean
) {
	const counts = Object.fromEntries(decisions.map((decision) => [decision, 0])) as Record<
		CommitteeDecision,
		number
	>;
	const models: Record<string, Record<ExpertDecision, number>> = {};
	const unknownReasons: Record<string, Record<string, number>> = {};
	const seen = new Set<number>();
	let disagreement = 0;
	let assessed = 0;
	for (const row of rows) {
		if (row.windowId === null || seen.has(row.windowId)) continue;
		seen.add(row.windowId);
		const committee = (row.statistical as StatisticalAssessment | null)?.committee;
		if (!committee || !decisions.includes(committee.decision) || !Array.isArray(committee.verdicts))
			continue;
		counts[committee.decision]++;
		assessed++;
		const active = new Set<ExpertDecision>();
		for (const item of committee.verdicts) {
			if (!item || typeof item.modelId !== 'string' || !verdicts.includes(item.decision)) continue;
			models[item.modelId] ??= Object.fromEntries(
				verdicts.map((decision) => [decision, 0])
			) as Record<ExpertDecision, number>;
			models[item.modelId][item.decision]++;
			if (item.decision === 'UNKNOWN') {
				const reasons = Array.isArray(item.reasons)
					? item.reasons.filter((r) => typeof r === 'string')
					: [];
				const counts = (unknownReasons[item.modelId] ??= {});
				for (const reason of new Set(reasons.length ? reasons : ['UNKNOWN_REASON']))
					counts[reason] = (counts[reason] ?? 0) + 1;
			}
			if (item.decision !== 'UNKNOWN') active.add(item.decision);
		}
		if (active.size > 1) disagreement++;
	}
	const latestLabel = new Map<string, string>();
	for (const row of labels)
		if (!latestLabel.has(row.caseId)) latestLabel.set(row.caseId, row.label);
	let confirmedAbuse = 0;
	let falsePositive = 0;
	for (const row of labels) {
		if ((row.statistical as StatisticalAssessment | null)?.committee?.decision !== 'KICK_CANDIDATE')
			continue;
		const latest = latestLabel.get(row.caseId);
		if (latest === 'CONFIRMED_ABUSE') confirmedAbuse++;
		if (latest === 'FALSE_POSITIVE') falsePositive++;
		latestLabel.delete(row.caseId);
	}
	return {
		counts,
		models,
		unknownReasons,
		assessed,
		disagreement,
		disagreementRate: assessed ? disagreement / assessed : null,
		confirmedAbuse,
		falsePositive,
		truncated
	};
}
