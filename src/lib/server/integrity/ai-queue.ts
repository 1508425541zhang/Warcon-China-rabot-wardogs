import { and, eq, inArray, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityAiJobs } from '../db/schema';
import { isOwner, withOwnedTransaction } from '../leadership';
import { ApiError } from '../http';
import { aiBundle, aiCall } from './ai';
import { reviewOutput, PROMPT_VERSION } from './ai-protocol';
let timer: ReturnType<typeof setInterval> | null = null;
let running: Promise<void> | null = null;
export async function discoverAiJobs(env: Env) {
	if (!isOwner()) return;
	await withOwnedTransaction(env, async (tx) => {
		await tx.execute(sql`INSERT INTO integrity_ai_jobs(case_id)
 SELECT c.id FROM integrity_cases c JOIN integrity_ai_settings s ON s.org_id=c.org_id
 JOIN organizations o ON o.id=c.org_id
 WHERE o.suspended_at IS NULL AND s.auto_enabled AND s.model <> '' AND c.reviewed_at IS NULL AND c.status='OPEN'
 AND c.created_at >= now()-interval '7 days'
 AND NOT EXISTS(SELECT 1 FROM integrity_ai_jobs j WHERE j.case_id=c.id)
 ORDER BY c.created_at,c.id LIMIT 100 ON CONFLICT DO NOTHING`);
		await tx.execute(sql`UPDATE integrity_ai_jobs j SET state='skipped', last_error='案件已人工审核或关闭',updated_at=now()
 FROM integrity_cases c WHERE j.case_id=c.id AND j.state='pending' AND (c.reviewed_at IS NOT NULL OR c.status<>'OPEN')`);
		await tx.execute(sql`UPDATE integrity_ai_jobs SET state='error',last_error='多次执行中断；停止自动重试',updated_at=now()
 WHERE state='running' AND lease_until<now() AND attempts>=3`);
	});
}
export async function processNextAiJob(env: Env, reviewer: typeof aiCall = aiCall) {
	if (!isOwner()) return false;
	const token = crypto.randomUUID();
	const job = await withOwnedTransaction(env, async (tx) => {
		const rows =
			await tx.execute(sql`UPDATE integrity_ai_jobs j SET state='running', attempts=attempts+1,
 claim_token=${token},lease_until=now()+interval '3 minutes',updated_at=now()
 WHERE j.case_id=(SELECT q.case_id FROM integrity_ai_jobs q
 JOIN integrity_cases c ON c.id=q.case_id JOIN integrity_ai_settings s ON s.org_id=c.org_id
 JOIN organizations o ON o.id=c.org_id
 WHERE o.suspended_at IS NULL AND s.auto_enabled AND s.model<>'' AND c.reviewed_at IS NULL AND c.status='OPEN'
 AND (s.last_request_at IS NULL OR s.last_request_at<now()-interval '65 seconds')
 AND (s.budget_day<>${new Date().toISOString().slice(0, 10)} OR s.daily_requests<s.daily_limit)
 AND q.attempts<3 AND ((q.state='pending' AND q.next_at<=now()) OR (q.state='running' AND q.lease_until<now()))
 ORDER BY q.next_at,c.created_at,q.case_id LIMIT 1 FOR UPDATE OF q SKIP LOCKED)
 RETURNING j.case_id AS "caseId", j.attempts`);
		return (rows as unknown as { caseId: string; attempts: number }[])[0];
	});
	if (!job) return false;
	try {
		const { integrityCases } = await import('../db/schema');
		const [c] = await env.db.select().from(integrityCases).where(eq(integrityCases.id, job.caseId));
		if (!c) return true;
		if (!isOwner()) return false;
		const response = await reviewer(
			env,
			c.orgId,
			'review',
			await aiBundle(env, c.orgId, c.serverId, c.id),
			true
		);
		if (!('result' in response)) throw new Error('missing review');
		await withOwnedTransaction(env, async (tx) => {
			await tx
				.update(integrityAiJobs)
				.set({
					state: 'done',
					result: response.result,
					lastError: null,
					leaseUntil: null,
					updatedAt: new Date()
				})
				.where(and(eq(integrityAiJobs.caseId, c.id), eq(integrityAiJobs.claimToken, token)));
		});
	} catch (error) {
		if (!isOwner()) return false;
		const deferred = error instanceof ApiError && [409, 429].includes(error.status);
		const exhausted =
			job.attempts >= 3 || (error instanceof ApiError && [400, 404, 413].includes(error.status));
		await withOwnedTransaction(env, async (tx) => {
			await tx
				.update(integrityAiJobs)
				.set({
					state: deferred || !exhausted ? 'pending' : 'error',
					attempts: deferred ? job.attempts - 1 : job.attempts,
					nextAt: new Date(Date.now() + (deferred ? 70000 : 300000 * job.attempts)),
					leaseUntil: null,
					lastError: error instanceof ApiError ? error.message : 'AI初审暂时失败，等待重试',
					updatedAt: new Date()
				})
				.where(and(eq(integrityAiJobs.caseId, job.caseId), eq(integrityAiJobs.claimToken, token)));
		});
	}
	return true;
}
export async function aiJobViews(env: Env, caseIds: string[]) {
	if (!caseIds.length) return [];
	const rows = await env.db
		.select()
		.from(integrityAiJobs)
		.where(inArray(integrityAiJobs.caseId, caseIds));
	return rows.map((row) => {
		const parsed = reviewOutput.safeParse(row.result);
		return {
			caseId: row.caseId,
			state: row.state,
			attempts: row.attempts,
			lastError: row.lastError,
			updatedAt: row.updatedAt.toISOString(),
			result: parsed.success ? parsed.data : null,
			promptVersion: PROMPT_VERSION
		};
	});
}
export function startIntegrityAi(env: Env) {
	if (timer) return;
	const pass = () => {
		if (running || !isOwner()) return;
		running = (async () => {
			await discoverAiJobs(env);
			await processNextAiJob(env);
		})()
			.catch(() => console.error('[warcon] AI queue pass failed'))
			.finally(() => {
				running = null;
			});
	};
	timer = setInterval(pass, 10000);
	pass();
}
export async function stopIntegrityAi() {
	if (timer) clearInterval(timer);
	timer = null;
	await running;
}
