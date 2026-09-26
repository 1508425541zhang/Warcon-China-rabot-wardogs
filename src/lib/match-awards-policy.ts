import { z } from 'zod';
export const awardKeys = ['mvp', 'multi', 'streak', 'earned', 'rich', 'time'] as const;
export type AwardKey = (typeof awardKeys)[number];
export const awardLabels: Record<AwardKey, string> = {
	mvp: '本局MVP',
	multi: '最佳单次多杀',
	streak: '最佳一命连杀',
	earned: '本场赚钱最多',
	rich: '本场最富玩家',
	time: '在线最久'
};
export const defaultAwards = {
	enabled: false,
	templates: {
		mvp: '本局MVP：{name}，KD {value}（{kills}杀/{deaths}死）',
		multi: '最佳单次多杀：{name}，同一时间戳 {value} 杀',
		streak: '最佳一命连杀：{name}，{value} 杀',
		earned: '本场赚钱最多（观测净增）：{name}，${value}',
		rich: '本场最富玩家：{name}，赛末观测持有 ${value}',
		time: '在线最久：{name}，{value} 分钟'
	}
};
export const awardsSchema = z
	.object({
		enabled: z.boolean(),
		templates: z
			.object({
				mvp: z.string().max(200),
				multi: z.string().max(200),
				streak: z.string().max(200),
				earned: z.string().max(200),
				rich: z.string().max(200),
				time: z.string().max(200)
			})
			.strict()
	})
	.strict();
export type AwardsConfig = z.infer<typeof awardsSchema>;
export interface AwardLine {
	name: string;
	steamId?: string;
	kills: number;
	deaths?: number;
	cashDelta?: number;
	cashHeld?: number;
	seconds?: number;
	multi?: number;
	streak?: number;
	awardSeed?: string;
}
function randomRank(seed: string): number {
	let n = 2166136261;
	for (const c of seed) n = Math.imul(n ^ c.charCodeAt(0), 16777619);
	n ^= n >>> 16;
	n = Math.imul(n, 0x85ebca6b);
	n ^= n >>> 13;
	return n >>> 0;
}
/** Round-seeded lottery: a single winner per award, stable across retries of the same round. */
export function awardWinners(lines: readonly AwardLine[]) {
	const value = (p: AwardLine, k: AwardKey) =>
		k === 'mvp'
			? p.deaths === undefined
				? undefined
				: p.kills / Math.max(1, p.deaths)
			: k === 'multi'
				? p.multi
				: k === 'streak'
					? p.streak
					: k === 'earned'
						? p.cashDelta
						: k === 'rich'
							? p.cashHeld
							: p.seconds;
	return awardKeys.flatMap((key) => {
		const ranked = lines
			.map((p) => ({ p, value: value(p, key) }))
			.filter(
				(x): x is { p: AwardLine; value: number } =>
					typeof x.value === 'number' &&
					Number.isFinite(x.value) &&
					x.value > 0 &&
					(key !== 'multi' || x.value >= 2)
			);
		ranked.sort(
			(a, b) =>
				b.value - a.value ||
				randomRank(`${a.p.awardSeed}:${key}:${a.p.steamId ?? a.p.name}`) -
					randomRank(`${b.p.awardSeed}:${key}:${b.p.steamId ?? b.p.name}`) ||
				(a.p.steamId ?? a.p.name).localeCompare(b.p.steamId ?? b.p.name)
		);
		const first = ranked[0];
		return first
			? [
					{
						key,
						player: first.p,
						value:
							key === 'mvp'
								? first.value.toFixed(2)
								: key === 'time'
									? (first.value / 60).toFixed(1)
									: String(first.value)
					}
				]
			: [];
	});
}
