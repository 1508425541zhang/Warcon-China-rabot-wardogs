import { and, eq, gte, lt, lte } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { kills } from '../db/schema';
import { countsAsInfantry, type WeaponCategory } from './weapons';
export const SUSTAINED_KPM_POLICY = 'consecutive-minute-v1';
export interface SustainedKpm {
	policyVersion: string;
	requiredMinutes: number;
	threshold: number;
	windows: { from: number; to: number; kills: number; kpm: number; exceeded: boolean }[];
	passed: boolean;
	reason: string;
}
/** Round-clock aligned closed minutes [from,to); never three overlapping rolling averages. */
export function consecutiveMinutes(
	clocks: number[],
	clock: number,
	minutes: number,
	threshold: number
): SustainedKpm {
	const end = Math.floor(clock / 60) * 60,
		start = end - minutes * 60;
	const windows = Array.from({ length: minutes }, (_, i) => {
		const from = start + i * 60,
			to = from + 60,
			kills = clocks.filter((t) => t >= from && t < to).length;
		return { from, to, kills, kpm: kills, exceeded: kills >= threshold };
	});
	const passed = start >= 0 && windows.every((w) => w.exceeded);
	return {
		policyVersion: SUSTAINED_KPM_POLICY,
		requiredMinutes: minutes,
		threshold,
		windows,
		passed,
		reason:
			start < 0 ? '完整分钟数不足' : passed ? '连续完整分钟均达到阈值' : '存在未达到阈值的分钟'
	};
}
export async function loadSustainedKpm(
	db: DbOrTx,
	input: {
		serverId: string;
		steamId: string;
		instanceId: string;
		roundId: string;
		clock: number;
		at: Date;
		minutes: number;
		threshold: number;
	},
	overrides: ReadonlyMap<string, WeaponCategory>
) {
	const result = consecutiveMinutes([], input.clock, input.minutes, input.threshold);
	const match = input.roundId.match(/:match:(\d+)$/);
	if (!match || result.windows[0].from < 0)
		return { ...result, reason: !match ? '轮次无法确认' : result.reason };
	const rows = await db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, input.serverId),
				eq(kills.instanceId, input.instanceId),
				eq(kills.matchRow, Number(match[1])),
				eq(kills.killerSteamId, input.steamId),
				gte(kills.eventTime, result.windows[0].from),
				lt(kills.eventTime, result.windows.at(-1)!.to),
				lte(kills.ts, input.at)
			)
		);
	const seen = new Set<string>(),
		clocks: number[] = [];
	for (const row of rows)
		if (
			!seen.has(row.eventId) &&
			countsAsInfantry(
				{
					...row,
					tags: Array.isArray(row.tags)
						? row.tags.filter((v): v is string => typeof v === 'string')
						: []
				},
				overrides
			)
		) {
			seen.add(row.eventId);
			clocks.push(row.eventTime);
		}
	return consecutiveMinutes(clocks, input.clock, input.minutes, input.threshold);
}
