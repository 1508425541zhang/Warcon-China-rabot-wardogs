import { createHash } from 'node:crypto';
import { and, desc, eq } from 'drizzle-orm';
import type { Env } from '../env';
import type { SessionUser } from '../access';
import { integrityImportBatches, integrityImportKills } from '../db/schema';
import { ApiError } from '../http';
import { writeAudit } from '../audit';
import { weaponOverrides } from './weapon-map';
import { classifyWeapon } from './weapons';
import { refreshIntegrityBaselines } from './baselines';

export const MAX_IMPORT_BYTES = 8 * 1024 * 1024;
export const MAX_IMPORT_ROWS = 10_000;
const STEAM = /^\d{17}$/;
const ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,79}$/;
const SOURCE = /^[A-Za-z0-9][A-Za-z0-9._-]{2,79}$/;
const DAYS = 30;

export interface ExternalKill {
	eventId: string;
	eventAt: Date;
	instanceId: string;
	matchId: string;
	eventTime: number;
	map: string;
	killerSteamId: string;
	victimSteamId: string;
	killerFaction: string;
	victimFaction: string;
	cause: string;
	distanceM: number | null;
	headshot: boolean;
	penetration: boolean;
	playerCount: number | null;
}

const asObject = (value: unknown, line: number): Record<string, unknown> => {
	if (!value || typeof value !== 'object' || Array.isArray(value))
		throw new ApiError(400, `第 ${line} 条必须是 JSON 对象。`);
	return value as Record<string, unknown>;
};

/** Canonical JSON array, {events: []}, or one JSON object per line. No guessed units or identities. */
export function parseExternalHistory(raw: string, now = new Date()): ExternalKill[] {
	if (!raw.trim() || Buffer.byteLength(raw, 'utf8') > MAX_IMPORT_BYTES || raw.includes('\uFFFD'))
		throw new ApiError(400, '文件为空、超过 8 MiB，或不是有效 UTF-8。');
	let input: unknown[];
	try {
		const parsed: unknown = JSON.parse(raw);
		input = Array.isArray(parsed)
			? parsed
			: Array.isArray((parsed as { events?: unknown } | null)?.events)
				? (parsed as { events: unknown[] }).events
				: [parsed];
	} catch {
		input = raw
			.split(/\r?\n/)
			.filter((line) => line.trim())
			.map((line, i) => {
				try {
					return JSON.parse(line) as unknown;
				} catch {
					throw new ApiError(400, `第 ${i + 1} 行不是合法 JSON。`);
				}
			});
	}
	if (!input.length || input.length > MAX_IMPORT_ROWS)
		throw new ApiError(400, `每个文件需要 1～${MAX_IMPORT_ROWS} 条记录。`);
	const seen = new Set<string>();
	return input.map((value, i) => {
		const line = i + 1;
		const row = asObject(value, line);
		const id = (key: string, pattern = ID) => {
			const item = row[key];
			if (typeof item !== 'string' || !pattern.test(item))
				throw new ApiError(400, `第 ${line} 条的 ${key} 无效。`);
			return item;
		};
		const eventId = id('eventId');
		if (seen.has(eventId)) throw new ApiError(400, `第 ${line} 条 eventId 重复。`);
		seen.add(eventId);
		const timestamp = row.eventAt;
		const eventAt =
			typeof timestamp === 'string' && /Z$/.test(timestamp) ? new Date(timestamp) : new Date(NaN);
		if (
			!Number.isFinite(eventAt.getTime()) ||
			eventAt.getTime() < now.getTime() - DAYS * 86_400_000 ||
			eventAt.getTime() > now.getTime() - 10 * 60_000
		)
			throw new ApiError(
				400,
				`第 ${line} 条 eventAt 必须是最近 ${DAYS} 天、至少 10 分钟前的 UTC 时间。`
			);
		const eventTime = row.eventTime;
		if (
			typeof eventTime !== 'number' ||
			!Number.isFinite(eventTime) ||
			eventTime < 0 ||
			eventTime > 86_400
		)
			throw new ApiError(400, `第 ${line} 条 eventTime 应为本局经过的秒数。`);
		const killerSteamId = id('killerSteamId', STEAM);
		const victimSteamId = id('victimSteamId', STEAM);
		if (killerSteamId === victimSteamId) throw new ApiError(400, `第 ${line} 条是自杀记录。`);
		const killerFaction = id('killerFaction');
		const victimFaction = id('victimFaction');
		if (killerFaction === victimFaction) throw new ApiError(400, `第 ${line} 条是同阵营记录。`);
		const cause = id('cause', /^Id\.Item\.[A-Za-z0-9_-]{1,80}$/);
		const distanceM = row.distanceM === null || row.distanceM === undefined ? null : row.distanceM;
		if (
			distanceM !== null &&
			(typeof distanceM !== 'number' || !Number.isFinite(distanceM) || distanceM <= 0)
		)
			throw new ApiError(400, `第 ${line} 条 distanceM 必须为米数或 null。`);
		if (typeof row.headshot !== 'boolean' || typeof row.penetration !== 'boolean')
			throw new ApiError(400, `第 ${line} 条 headshot、penetration 必须为布尔值。`);
		const playerCount =
			row.playerCount === null || row.playerCount === undefined ? null : row.playerCount;
		if (
			playerCount !== null &&
			(!Number.isInteger(playerCount) || Number(playerCount) < 1 || Number(playerCount) > 200)
		)
			throw new ApiError(400, `第 ${line} 条 playerCount 应为 1～200 的整数或 null。`);
		return {
			eventId,
			eventAt,
			instanceId: id('instanceId'),
			matchId: id('matchId'),
			eventTime,
			map: id('map'),
			killerSteamId,
			victimSteamId,
			killerFaction,
			victimFaction,
			cause,
			distanceM: distanceM as number | null,
			headshot: row.headshot as boolean,
			penetration: row.penetration as boolean,
			playerCount: playerCount as number | null
		};
	});
}

export async function stageIntegrityImport(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	sourceServer: string,
	raw: string
) {
	if (!SOURCE.test(sourceServer))
		throw new ApiError(400, '来源服务器标识只能包含 3～80 位字母、数字、点、下划线和连字符。');
	const rows = parseExternalHistory(raw);
	const overrides = await weaponOverrides(env, orgId);
	for (const [i, row] of rows.entries())
		if (classifyWeapon({ cause: row.cause, tags: [], suicide: false }, overrides) !== 'INFANTRY')
			throw new ApiError(400, `第 ${i + 1} 条 cause 不是已确认的步兵枪械；请先在武器映射中审核。`);
	const hash = createHash('sha256').update(raw).digest('hex');
	const [existing] = await env.db
		.select({ id: integrityImportBatches.id })
		.from(integrityImportBatches)
		.where(
			and(eq(integrityImportBatches.orgId, orgId), eq(integrityImportBatches.fileSha256, hash))
		)
		.limit(1);
	if (existing) throw new ApiError(409, `文件已上传，批次 ${existing.id}。`);
	const id = crypto.randomUUID();
	const dates = rows.map((row) => row.eventAt.getTime());
	await env.db.transaction(async (tx) => {
		await tx.insert(integrityImportBatches).values({
			id,
			orgId,
			sourceServer,
			fileSha256: hash,
			status: 'STAGED',
			rowCount: rows.length,
			firstEventAt: new Date(Math.min(...dates)),
			lastEventAt: new Date(Math.max(...dates)),
			stagedBy: actor.id
		});
		for (let start = 0; start < rows.length; start += 500)
			await tx.insert(integrityImportKills).values(
				rows.slice(start, start + 500).map((row) => ({
					...row,
					batchId: id,
					orgId,
					sourceServer
				}))
			);
	});
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.import.stage',
		outcome: 'ok',
		message: `Staged ${rows.length} external kill events from ${sourceServer}`,
		detail: { batchId: id, sha256: hash, rows: rows.length }
	});
	return {
		id,
		sourceServer,
		status: 'STAGED',
		rowCount: rows.length,
		firstEventAt: new Date(Math.min(...dates)).toISOString(),
		lastEventAt: new Date(Math.max(...dates)).toISOString(),
		weapons: [...new Set(rows.map((row) => row.cause))].sort(),
		maps: [...new Set(rows.map((row) => row.map))].sort(),
		populationKnown: rows.filter((row) => row.playerCount !== null).length
	};
}

export async function integrityImports(env: Env, orgId: string) {
	return env.db
		.select()
		.from(integrityImportBatches)
		.where(eq(integrityImportBatches.orgId, orgId))
		.orderBy(desc(integrityImportBatches.stagedAt))
		.limit(50);
}

export async function reviewIntegrityImport(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	id: string,
	decision: 'APPROVED' | 'REJECTED'
) {
	const [batch] = await env.db.transaction(async (tx) => {
		const [current] = await tx
			.select()
			.from(integrityImportBatches)
			.where(and(eq(integrityImportBatches.id, id), eq(integrityImportBatches.orgId, orgId)))
			.for('update')
			.limit(1);
		if (!current) throw new ApiError(404, '导入批次不存在。');
		if (current.status === decision || current.status === 'REJECTED')
			throw new ApiError(409, '该批次已审核，不能重复批准。');
		return tx
			.update(integrityImportBatches)
			.set({ status: decision, reviewedAt: new Date(), reviewedBy: actor.id })
			.where(eq(integrityImportBatches.id, id))
			.returning();
	});
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: `integrity.import.${decision.toLowerCase()}`,
		outcome: 'ok',
		message: `${decision} external history ${id}`,
		detail: { batchId: id, sourceServer: batch.sourceServer, rows: batch.rowCount }
	});
	await refreshIntegrityBaselines(env, orgId);
	return batch;
}
