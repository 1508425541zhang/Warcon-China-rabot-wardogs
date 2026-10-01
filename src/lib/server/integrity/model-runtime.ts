import { longModelEnabled } from '$lib/integrity-engines';
import { getIntegrityRules } from './rules';
import { sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import type { Env } from '../env';
import {
	modelConfig,
	callModel,
	MODEL_SCHEMA,
	MODEL_CALIBRATION,
	validateModelResult
} from './model-http';
import { enforceModelRun } from './model-enforcement';
import { isOwner } from '../leadership';
import { ApiError } from '../http';
import { allMemory } from '../observe';

/** Export one player's observed features only; no credentials, names, labels or expert opinions. */
export async function modelSources(
	env: Env,
	serverId: string,
	steamId: string,
	matchId: number,
	at: Date
) {
	const match = await env.db.execute(
		sql`SELECT id, started_at, ${at.toISOString()}::timestamptz AS ended_at, map FROM matches WHERE id=${matchId} AND server_id=${serverId} AND started_at < ${at}`
	);
	if (!match.length) throw new ApiError(404, '模型所需对局不存在。');
	// Thirty minutes plus counter context; preserve sixty ordered thirty-second buckets.
	const since = new Date(at.getTime() - 1900_000);
	const progress = await env.db.execute(sql`SELECT server_id, match_id, observed_at,
  jsonb_array_length(players) AS roster_size,
  (SELECT jsonb_agg(jsonb_build_object('steamId','player','cash',p->'cash','kills',p->'kills','deaths',p->'deaths')) FROM jsonb_array_elements(players) p WHERE p->>'steamId'=${steamId}) AS players
  FROM player_progress_samples WHERE server_id=${serverId} AND match_id=${matchId} AND observed_at>=${since} AND observed_at<${at}
  AND EXISTS(SELECT 1 FROM jsonb_array_elements(players) p WHERE p->>'steamId'=${steamId}) ORDER BY observed_at LIMIT 25001`);
	const history = await env.db
		.execute(sql`SELECT server_id,round_id,observed_at,'player' AS steam_id,kpm_180,headshot_rate,max_kills_15s
  FROM integrity_player_metric_history WHERE server_id=${serverId} AND steam_id=${steamId} AND round_id LIKE ${'%:match:' + matchId}
  AND observed_at>=${since} AND observed_at<${at} ORDER BY observed_at,id LIMIT 25001`);
	const windows = await env.db
		.execute(sql`SELECT server_id,round_id,observed_at,'player' AS steam_id,infantry_kills,kpm_180,unique_victims,headshots,penetrations,burst_points,max_kills_15s,median_kill_interval
  FROM integrity_windows WHERE server_id=${serverId} AND steam_id=${steamId} AND round_id LIKE ${'%:match:' + matchId}
  AND observed_at>=${since} AND observed_at<${at} ORDER BY observed_at,id LIMIT 25001`);
	if ([progress, history, windows].some((rows) => rows.length > 25000))
		throw new ApiError(413, '模型输入超限，未截断数据。');
	return {
		matches: match,
		player_progress_samples: progress,
		integrity_player_metric_history: history,
		integrity_windows: windows
	};
}

export async function runPlayerModel(
	env: Env,
	orgId: string,
	serverId: string,
	steamId: string,
	matchId: number,
	at = new Date()
) {
	const config = await modelConfig(env, orgId);
	if (!config.developerEnabled) return;
	const id = randomUUID();
	const slot = Math.floor(at.getTime() / (config.intervalSeconds * 1000));
	await env.db
		.execute(sql`INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold)
  SELECT ${id},${orgId},${serverId},${steamId},${matchId},${slot},${config.revision},${MODEL_CALIBRATION.p98}
  WHERE EXISTS(SELECT 1 FROM servers s JOIN integrity_rules r ON r.org_id=s.org_id WHERE s.id=${serverId} AND s.org_id=${orgId} AND r.assessment_mode IN ('model_only','long_only'))
  AND EXISTS(SELECT 1 FROM matches WHERE id=${matchId} AND server_id=${serverId} AND started_at<=${at.toISOString()}::timestamptz-interval '30 minutes')
  AND NOT EXISTS(SELECT 1 FROM integrity_model_runs WHERE server_id=${serverId} AND steam_id=${steamId} AND match_id=${matchId} AND config_revision=${config.revision} AND created_at>${at.toISOString()}::timestamptz-(${config.intervalSeconds}*interval '1 second'))
  ON CONFLICT DO NOTHING RETURNING id`);
}

export async function processModelQueue(env: Env) {
	// Discard invalid configurations together so they cannot delay the current queue.
	await env.db.execute(sql`UPDATE integrity_model_runs j
  SET state='superseded',finished_at=now(),result=jsonb_build_object('reason','模型配置已更新或长时序评估已停用，未执行评分。')
  WHERE j.state='pending' AND NOT EXISTS (
    SELECT 1 FROM site_settings s JOIN integrity_rules r ON r.org_id=j.org_id
    WHERE s.key='integrityModel:'||j.org_id AND s.value->>'developerEnabled'='true'
    AND s.value->>'revision'=j.config_revision AND r.assessment_mode IN ('model_only','long_only')
  )`);
	const recover = await env.db.execute<{ id: string }>(
		sql`SELECT id FROM integrity_model_runs WHERE state='READY' AND action_state IS NULL ORDER BY created_at LIMIT 5`
	);
	for (const row of recover) await enforceModelRun(env, row.id);
	await env.db.execute(
		sql`UPDATE integrity_model_runs SET state='ERROR',finished_at=now() WHERE state='running' AND created_at<now()-interval '5 minutes'`
	);
	const [job] = await env.db.execute<{
		id: string;
		org_id: string;
		server_id: string;
		steam_id: string;
		match_id: number;
		config_revision: string;
		created_at: Date;
	}>(sql`
  UPDATE integrity_model_runs SET state='running' WHERE id=(SELECT id FROM integrity_model_runs WHERE state='pending' ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING *`);
	if (!job) return false;
	const { id, org_id: orgId, server_id: serverId, steam_id: steamId, match_id: matchId } = job;
	try {
		const config = await modelConfig(env, orgId);
		const active = await env.db.execute(
			sql`SELECT 1 FROM integrity_rules WHERE org_id=${orgId} AND assessment_mode IN ('model_only','long_only')`
		);
		if (!config.developerEnabled || config.revision !== job.config_revision || !active.length) {
			await env.db.execute(
				sql`UPDATE integrity_model_runs SET state='superseded',finished_at=now(),result=jsonb_build_object('reason','模型配置已更新或长时序评估已停用，未执行评分。') WHERE id=${id}`
			);
			return true;
		}
		// Keep the queued observation window for reporting, even after a backlog.
		// enforceModelRun independently prevents punishment from scores older than five minutes.
		const sources = await modelSources(
			env,
			serverId,
			steamId,
			Number(matchId),
			new Date(job.created_at)
		);
		const output = validateModelResult(
			await callModel(env, config, { schema: MODEL_SCHEMA, requestId: id, sources }),
			id
		);
		const current = await modelConfig(env, orgId);
		const mode = await env.db.execute(
			sql`SELECT 1 FROM integrity_rules WHERE org_id=${orgId} AND assessment_mode IN ('model_only','long_only')`
		);
		const stale = !current.developerEnabled || current.revision !== config.revision || !mode.length;
		const detail = stale
			? { ...output.detail, reason: '评分期间模型配置或评估模式已变更，结果不用于处罚。' }
			: output.detail;
		await env.db.execute(
			sql`UPDATE integrity_model_runs SET state=${stale ? 'superseded' : output.status},score=${output.score},result=${JSON.stringify(detail)}::text::jsonb,finished_at=now() WHERE id=${id}`
		);
		if (!stale && output.status === 'READY') await enforceModelRun(env, id);
	} catch {
		await env.db.execute(
			sql`UPDATE integrity_model_runs SET state='ERROR',result='{"message":"模型请求或处置失败，请检查处罚状态；未回退专家。"}'::jsonb,finished_at=now() WHERE id=${id} AND punished_at IS NULL`
		);
	}
	return true;
}

let timer: ReturnType<typeof setInterval> | undefined;
let pending: Promise<void> | undefined;
let lastSweep = 0;
export async function scheduleOnlineModels(env: Env, at = new Date()) {
	for (const memory of allMemory()) {
		if (
			!memory.ok ||
			at.getTime() - memory.playersAt > 30000 ||
			at.getTime() - memory.statusAt > 30000
		)
			continue;
		if (!longModelEnabled((await getIntegrityRules(env, memory.server.orgId)).assessmentMode))
			continue;
		const config = await modelConfig(env, memory.server.orgId);
		if (!config.developerEnabled) continue;
		const [match] = await env.db.execute<{ id: number }>(
			sql`SELECT id FROM matches WHERE server_id=${memory.server.id} AND ended_at IS NULL AND started_at<=${at.toISOString()}::timestamptz-interval '30 minutes' ORDER BY started_at DESC LIMIT 1`
		);
		if (!match) continue;
		for (const player of memory.players)
			await runPlayerModel(
				env,
				memory.server.orgId,
				memory.server.id,
				player.steamId,
				Number(match.id),
				at
			);
	}
}
export function startModelQueue(env: Env) {
	if (timer) return;
	timer = setInterval(() => {
		if (!pending && isOwner())
			pending = (async () => {
				const active = await env.db.execute(
					sql`SELECT 1 FROM integrity_rules r JOIN site_settings s ON s.key='integrityModel:'||r.org_id WHERE r.assessment_mode IN ('model_only','long_only') AND s.value->>'developerEnabled'='true' LIMIT 1`
				);
				if (!active.length) return;
				if (Date.now() - lastSweep >= 10000) {
					await scheduleOnlineModels(env);
					lastSweep = Date.now();
				}
				for (let i = 0; i < 20; i++) {
					if (!(await processModelQueue(env))) break;
				}
			})()
				.catch(() => {})
				.finally(() => {
					pending = undefined;
				});
	}, 10000);
}
export async function stopModelQueue() {
	clearInterval(timer);
	timer = undefined;
	await pending;
}

export async function modelRuns(env: Env, orgId: string, serverId?: string) {
	return env.db
		.execute(sql`SELECT id,server_id,steam_id,match_id,state,score,threshold,created_at,finished_at,action,action_state,action_reason,expires_at,
  result->>'reason' AS data_reason, result->>'windowSeconds' AS window_seconds,
  result->>'observedBuckets' AS observed_buckets, result->>'requiredBuckets' AS required_buckets,
  CASE WHEN state='READY' THEN score>=threshold ELSE NULL END AS anomalous
  FROM integrity_model_runs WHERE org_id=${orgId} AND ${serverId ? sql`server_id=${serverId}` : sql`true`} ORDER BY created_at DESC LIMIT 50`);
}
