import { and, eq, inArray, sql } from 'drizzle-orm';
import type { Env } from './env';
import { feedProcessingJobs, kills, type FeedProcessingJob } from './db/schema';
import { killView } from './feed';
import { onKillsIngested } from './feed-events';
import { processIntegrityBatch } from './integrity/pipeline';
import { isOwner, LostOwnership, withOwnedTransaction } from './leadership';
import {
	feedJobsOldestSeconds,
	feedJobsPending,
	feedJobsProcessing,
	feedJobsRetrying,
	registerCollector
} from './metrics';

const LEASE_MS = 120_000;
const LIVE_MS = 5 * 60_000;
const RETRY_MS = 5_000;
const MAX_SAFE_BACKLOG = 500;
let envRef: Env | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
type FeedConsumer = 'legacy' | 'integrity';
const running = new Map<FeedConsumer, Promise<void>>();
let unregisterMetrics: (() => void) | null = null;

export interface FeedJobDepth {
	pending: number;
	processing: number;
	retrying: number;
	oldestMs: number | null;
}
export async function feedJobDepth(env: Env, consumer?: FeedConsumer): Promise<FeedJobDepth> {
	const [row] = (await env.db.execute(sql`
		SELECT COUNT(*) FILTER (WHERE state = 'pending')::int AS pending,
		       COUNT(*) FILTER (WHERE state = 'processing')::int AS processing,
		       COUNT(*) FILTER (WHERE attempts > 1 OR last_error IS NOT NULL)::int AS retrying,
		       MIN(created_at) AS oldest
		FROM feed_processing_jobs WHERE state IN ('pending', 'processing')
		  AND (${consumer === undefined ? sql`TRUE` : sql`consumer = ${consumer}`})
	`)) as { pending: number; processing: number; retrying: number; oldest: Date | null }[];
	return {
		pending: Number(row?.pending ?? 0),
		processing: Number(row?.processing ?? 0),
		retrying: Number(row?.retrying ?? 0),
		oldestMs: row?.oldest ? Math.max(0, Date.now() - new Date(row.oldest).getTime()) : null
	};
}

export const feedBacklogUnsafe = (depth: FeedJobDepth): boolean =>
	depth.pending >= MAX_SAFE_BACKLOG || (depth.oldestMs !== null && depth.oldestMs > LIVE_MS);

/** One claimed job; an expired lease can be claimed again after a worker crash. */
export async function claimFeedJob(
	env: Env,
	onlyId?: number,
	consumer: FeedConsumer = 'legacy'
): Promise<FeedProcessingJob | null> {
	if (!isOwner()) return null;
	return withOwnedTransaction(env, async (tx) => {
		const [job] = (await tx.execute(sql`
			UPDATE feed_processing_jobs SET state = 'processing', attempts = attempts + 1,
				lease_until = now() + (${LEASE_MS} || ' milliseconds')::interval
			WHERE id = (
				SELECT j.id FROM feed_processing_jobs j
				WHERE j.consumer = ${consumer}
				  AND (${onlyId === undefined ? sql`TRUE` : sql`j.id = ${onlyId}`}) AND (
				   (j.state = 'pending' AND (j.lease_until IS NULL OR j.lease_until <= now()))
				   OR (j.state = 'processing' AND j.lease_until <= now()))
				  AND NOT EXISTS (SELECT 1 FROM feed_processing_jobs earlier
				      WHERE earlier.consumer = j.consumer AND earlier.server_id = j.server_id
				        AND earlier.state <> 'done'
				        AND (earlier.created_at, earlier.id) < (j.created_at, j.id))
				ORDER BY j.created_at, j.id LIMIT 1 FOR UPDATE OF j SKIP LOCKED
			)
			RETURNING id, server_id AS "serverId", kill_ts AS "killTs", event_ids AS "eventIds",
				consumer, created_at AS "createdAt", state, attempts, lease_until AS "leaseUntil",
				done_at AS "doneAt", last_error AS "lastError"
		`)) as FeedProcessingJob[];
		return job ?? null;
	});
}

/** Run exactly the persisted batch. Missing source events keep the job retryable. */
export async function processFeedJob(env: Env, job: FeedProcessingJob): Promise<void> {
	const ids = job.eventIds;
	if (!Array.isArray(ids) || !ids.length || ids.some((id) => typeof id !== 'string'))
		throw new Error(`Feed job ${job.id} has invalid event IDs`);
	const rows = await env.db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, job.serverId),
				eq(kills.ts, job.killTs),
				inArray(kills.eventId, ids as string[])
			)
		);
	const byId = new Map(rows.map((row) => [row.eventId, row]));
	if (byId.size !== ids.length) throw new Error(`Feed job ${job.id} is missing persisted kills`);
	const batch = (ids as string[]).map((id) => killView(byId.get(id)!));
	// A delayed replay still fills evidence, but cannot punish today's player for old kills.
	const healthy = !feedBacklogUnsafe(await feedJobDepth(env, job.consumer as FeedConsumer));
	const allowActions = healthy && Date.now() - job.createdAt.getTime() <= LIVE_MS;
	if (job.consumer === 'integrity')
		await processIntegrityBatch(env, job.serverId, batch, allowActions);
	else await onKillsIngested(env, job.serverId, batch, allowActions);
}

export async function processNextFeedJob(
	env: Env,
	onlyId?: number,
	consumer: FeedConsumer = 'legacy'
): Promise<boolean> {
	const job = await claimFeedJob(env, onlyId, consumer);
	if (!job) return false;
	try {
		await processFeedJob(env, job);
		await withOwnedTransaction(env, async (tx) => {
			const [done] = await tx
				.update(feedProcessingJobs)
				.set({ state: 'done', doneAt: new Date(), leaseUntil: null, lastError: null })
				.where(
					and(
						eq(feedProcessingJobs.id, job.id),
						eq(feedProcessingJobs.state, 'processing'),
						eq(feedProcessingJobs.attempts, job.attempts)
					)
				)
				.returning({ id: feedProcessingJobs.id });
			if (!done) throw new Error(`Feed job ${job.id} lease changed before completion`);
		});
	} catch (err) {
		if (err instanceof LostOwnership) throw err;
		console.error(`[warcon] feed job ${job.id}:`, err);
		await withOwnedTransaction(env, async (tx) => {
			await tx
				.update(feedProcessingJobs)
				.set({
					state: 'pending',
					leaseUntil: new Date(Date.now() + RETRY_MS),
					lastError: String(err instanceof Error ? err.message : err).slice(0, 500)
				})
				.where(
					and(eq(feedProcessingJobs.id, job.id), eq(feedProcessingJobs.attempts, job.attempts))
				);
		});
	}
	return true;
}

const pass = (consumer: FeedConsumer) => {
	if (!envRef || running.has(consumer) || !isOwner()) return;
	const current = Promise.all(
		Array.from({ length: 4 }, async () => {
			for (let i = 0; i < 20 && envRef && isOwner(); i++)
				if (!(await processNextFeedJob(envRef, undefined, consumer))) break;
		})
	)
		.then(() => {})
		.catch((err) => console.error('[warcon] feed processing pass', err))
		.finally(() => {
			running.delete(consumer);
		});
	running.set(consumer, current);
};

export function startFeedProcessing(env: Env): void {
	envRef = env;
	if (timer) clearInterval(timer);
	unregisterMetrics?.();
	unregisterMetrics = registerCollector(async () => {
		const depth = await feedJobDepth(env);
		feedJobsPending.set(depth.pending);
		feedJobsProcessing.set(depth.processing);
		feedJobsRetrying.set(depth.retrying);
		feedJobsOldestSeconds.set((depth.oldestMs ?? 0) / 1000);
	});
	timer = setInterval(() => {
		pass('legacy');
		pass('integrity');
	}, 1000);
	pass('legacy');
	pass('integrity');
}

export function wakeFeedProcessing(): void {
	pass('legacy');
	pass('integrity');
}

export async function stopFeedProcessing(): Promise<void> {
	if (timer) clearInterval(timer);
	timer = null;
	envRef = null;
	unregisterMetrics?.();
	unregisterMetrics = null;
	await Promise.all([...running.values()]);
}
