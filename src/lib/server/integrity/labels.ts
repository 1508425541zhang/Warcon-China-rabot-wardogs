import { and, eq } from 'drizzle-orm';
import type { Env } from '../env';
import type { SessionUser } from '../access';
import { integrityCases, integrityLabels } from '../db/schema';
import { ApiError } from '../http';
import { writeAudit } from '../audit';

export const INTEGRITY_LABELS = [
	'FALSE_POSITIVE',
	'CONFIRMED_ABUSE',
	'INSUFFICIENT_EVIDENCE',
	'DATA_ERROR'
] as const;
export type IntegrityLabel = (typeof INTEGRITY_LABELS)[number];

/** Human correction is append-only; it never mutates a live score or normal-career distribution. */
export async function labelIntegrityCase(
	env: Env,
	request: Request,
	actor: SessionUser,
	orgId: string,
	caseId: string,
	label: unknown,
	reason: unknown
) {
	if (!INTEGRITY_LABELS.includes(label as IntegrityLabel))
		throw new ApiError(400, '无效的案件标签。');
	if (typeof reason !== 'string' || reason.trim().length < 5 || reason.length > 2000)
		throw new ApiError(400, '请填写 5～2000 字的审核理由。');
	const [caseRow] = await env.db
		.select()
		.from(integrityCases)
		.where(and(eq(integrityCases.id, caseId), eq(integrityCases.orgId, orgId)))
		.limit(1);
	if (!caseRow) throw new ApiError(404, '案件不存在。');
	const statistical = caseRow.statistical as { modelVersion?: unknown } | null;
	const modelVersion =
		typeof statistical?.modelVersion === 'string'
			? statistical.modelVersion
			: `legacy-rules-${caseRow.ruleVersion}`;
	const [row] = await env.db
		.insert(integrityLabels)
		.values({
			caseId,
			orgId,
			label: label as IntegrityLabel,
			reason: reason.trim(),
			reviewerId: actor.id,
			modelVersion
		})
		.returning();
	await writeAudit(env, request, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.case.label',
		outcome: 'ok',
		message: `${label} label recorded for ${caseId}`,
		detail: { caseId, label, labelId: row.id }
	});
	return row;
}
