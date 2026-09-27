import { and, eq, sql } from 'drizzle-orm';
import type { Tx } from '../db';
import {
	integrityAiSettings,
	integrityAiJobs,
	integrityAiReviews,
	integrityCases,
	organizations
} from '../db/schema';
import { PROMPT_VERSION, reviewOutput, numericChecks } from './ai-protocol';

/** Model recommendations never call the human label/ban path. */
export function triageDecision(input: unknown, deleteLowRisk: boolean) {
	const r = reviewOutput.parse(input);
	if (r.suspicionPercent !== null && r.suspicionPercent >= 65) return 'ADMIN_REVIEW' as const;
	if (
		r.suspicionPercent === null ||
		r.verdict === '证据不足' ||
		r.evidenceQuality === '低' ||
		r.contradictions.length ||
		!r.reasons.length ||
		r.reasons.some((r) => !r.evidence.trim())
	)
		return 'AI_ARCHIVED_UNRESOLVED' as const;
	if (
		deleteLowRisk &&
		r.suspicionPercent <= 25 &&
		r.verdict === '建议通过' &&
		!r.missingEvidence.length
	)
		return 'AI_CLEARED' as const;
	return 'AI_ARCHIVED' as const;
}

/** Lock in case→settings→job order; a human review during the request always wins. */
export async function finishAiReview(
	tx: Tx,
	caseId: string,
	token: string,
	input: unknown,
	settingsVersion: Date
) {
	const parsed = reviewOutput.parse(input);
	const [c] = await tx
		.select()
		.from(integrityCases)
		.where(eq(integrityCases.id, caseId))
		.for('update');
	if (!c) return;
	const [settings] = await tx
		.select()
		.from(integrityAiSettings)
		.where(eq(integrityAiSettings.orgId, c.orgId))
		.for('share');
	const [org] = await tx
		.select()
		.from(organizations)
		.where(eq(organizations.id, c.orgId))
		.for('share');
	const [job] = await tx
		.select()
		.from(integrityAiJobs)
		.where(and(eq(integrityAiJobs.caseId, caseId), eq(integrityAiJobs.claimToken, token)))
		.for('update');
	if (!job || job.state !== 'running' || !job.leaseUntil || job.leaseUntil <= new Date()) return;
	if (
		!settings?.autoEnabled ||
		!org ||
		org.suspendedAt ||
		settings.updatedAt.getTime() !== settingsVersion.getTime()
	) {
		await tx
			.update(integrityAiJobs)
			.set({
				state: 'pending',
				leaseUntil: null,
				attempts: 0,
				nextAt: new Date(),
				lastError: '配置已变更，等待按最新配置重新审核'
			})
			.where(eq(integrityAiJobs.caseId, caseId));
		return;
	}
	let disposition: string = 'ADVISORY';
	if (c.status !== 'OPEN' || c.reviewedAt) disposition = 'SKIPPED_REVIEWED';
	else if (settings.autoCloseEnabled) {
		disposition =
			numericChecks(c.snapshot).kpm180.matches === false
				? parsed.suspicionPercent !== null && parsed.suspicionPercent >= 65
					? 'ADMIN_REVIEW'
					: 'AI_ARCHIVED_UNRESOLVED'
				: triageDecision(parsed, settings.deleteLowRisk);
		// These records remain available for appeals and pending/previous actions.
		const [protectedCase] = (await tx.execute(sql`SELECT EXISTS (
		 SELECT 1 FROM integrity_actions WHERE case_id=${caseId}
		 UNION ALL SELECT 1 FROM integrity_labels WHERE case_id=${caseId}
		 UNION ALL SELECT 1 FROM integrity_reports WHERE case_id=${caseId}
		 UNION ALL SELECT 1 FROM integrity_action_eligibility WHERE case_id=${caseId}
		) AS protected`)) as unknown as { protected: boolean }[];
		if (protectedCase.protected) disposition = 'ADMIN_REVIEW';
		if (
			disposition === 'AI_ARCHIVED' ||
			disposition === 'AI_CLEARED' ||
			disposition === 'AI_ARCHIVED_UNRESOLVED'
		) {
			await tx
				.update(integrityCases)
				.set({
					status: disposition === 'AI_CLEARED' ? 'AI_CLEARED' : 'AI_ARCHIVED',
					reviewedAt: new Date(),
					reviewedBy: null
				})
				.where(eq(integrityCases.id, caseId));
		}
		if (disposition === 'AI_CLEARED') {
			// Delete bulky evidence copies; keep only receipt/dedup and baseline exclusion keys.
			// Original kills, scores and telemetry are never removed or labelled clean by AI.
			await tx.execute(sql`UPDATE integrity_cases SET snapshot=jsonb_build_object(
			 'roundId',snapshot->'roundId','eventIds',snapshot->'eventIds'),
			 statistical=CASE WHEN statistical IS NULL THEN NULL ELSE jsonb_build_object('level',statistical->'level') END,
			 risk_breakdown='[]'::jsonb WHERE id=${caseId}`);
			await tx.execute(sql`UPDATE integrity_case_events SET event=jsonb_build_object(
			 'killerSteamId',event->'killerSteamId','ts',event->'ts') WHERE case_id=${caseId}`);
			await tx.delete(integrityAiReviews).where(eq(integrityAiReviews.caseId, caseId));
		}
	}
	const result = {
		...parsed,
		promptVersion: PROMPT_VERSION,
		disposition,
		...(disposition === 'AI_CLEARED'
			? { reasons: [], alternatives: [], summary: parsed.summary.slice(0, 300) }
			: {}),
		model:
			typeof (input as { model?: unknown })?.model === 'string'
				? (input as { model: string }).model
				: settings.model
	};
	await tx
		.update(integrityAiJobs)
		.set({ state: 'done', result, lastError: null, leaseUntil: null, updatedAt: new Date() })
		.where(eq(integrityAiJobs.caseId, caseId));
}
