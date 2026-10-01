import { z } from 'zod';
import type { FactionScore, Player } from './types';

export const quotaColors = ['blue', 'red', 'green'] as const;
export type QuotaColor = (typeof quotaColors)[number];
export const quotaLabels: Record<QuotaColor, string> = { blue: '蓝方', red: '红方', green: '绿方' };
export const quotaInput = z
	.object({
		revision: z.string().max(100),
		enabled: z.boolean(),
		limits: z.object({
			blue: z.number().int().min(0).max(100),
			red: z.number().int().min(0).max(100),
			green: z.number().int().min(0).max(100)
		}),
		graceSeconds: z.number().int().min(30).max(600).default(60)
	})
	.refine(
		(c) => quotaColors.filter((color) => c.limits[color] > 0).length >= 2,
		'至少开放两个阵营。'
	)
	.refine(
		(c) => quotaColors.reduce((n, color) => n + c.limits[color], 0) <= 100,
		'三方人数合计不能超过100。'
	);

/** Match actual scoreboard colours, never scoreboard order or a translated faction name. */
export function quotaTeams(scores: readonly FactionScore[]): Record<QuotaColor, string> | null {
	const result: Partial<Record<QuotaColor, string>> = {};
	for (const score of scores) {
		const hex = score.colorHex.replace(/^#/, '');
		if (!/^[0-9a-f]{6}$/i.test(hex)) return null;
		const rgb = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16));
		const max = Math.max(...rgb),
			min = Math.min(...rgb);
		if (max - min < 40 || rgb.filter((v) => v === max).length !== 1) return null;
		const color: QuotaColor = rgb[0] === max ? 'red' : rgb[1] === max ? 'green' : 'blue';
		if (!score.name || result[color]) return null;
		result[color] = score.name;
	}
	return quotaColors.every((c) => result[c]) && new Set(Object.values(result)).size === 3
		? (result as Record<QuotaColor, string>)
		: null;
}

export type QuotaMove = { steamId: string; from: string; to: string };
export function planQuota(
	players: readonly Player[],
	teams: Record<QuotaColor, string>,
	limits: Record<QuotaColor, number>,
	blockedIds: ReadonlySet<string> = new Set()
) {
	const roster = players.filter((p) => quotaColors.some((c) => teams[c] === p.faction));
	const counts = Object.fromEntries(
		quotaColors.map((c) => [c, roster.filter((p) => p.faction === teams[c]).length])
	) as Record<QuotaColor, number>;
	const total = quotaColors.reduce((n, c) => n + limits[c], 0);
	if (roster.length > total)
		return {
			counts,
			targets: limits,
			moves: [] as QuotaMove[],
			reason: '在线阵营人数超过配额总和，暂无可用位置；不会踢人腾位。'
		};
	const raw = quotaColors.map((c) => ({ c, value: (roster.length * limits[c]) / total }));
	const targets = Object.fromEntries(raw.map(({ c, value }) => [c, Math.floor(value)])) as Record<
		QuotaColor,
		number
	>;
	let rest = roster.length - quotaColors.reduce((n, c) => n + targets[c], 0);
	// On an odd population, preserve the current larger side rather than moving the odd player back and forth.
	for (const { c } of [...raw].sort(
		(a, b) =>
			(b.value % 1) - (a.value % 1) ||
			counts[b.c] - counts[a.c] ||
			quotaColors.indexOf(a.c) - quotaColors.indexOf(b.c)
	)) {
		if (rest > 0 && targets[c] < limits[c]) {
			targets[c]++;
			rest--;
		}
	}
	const projected = { ...counts },
		moves: QuotaMove[] = [];
	for (const from of quotaColors) {
		const candidates = roster
			.filter(
				(p) => p.faction === teams[from] && /^\d{17}$/.test(p.steamId) && !blockedIds.has(p.steamId)
			)
			.sort((a, b) => a.kills - b.kills || a.steamId.localeCompare(b.steamId));
		for (const player of candidates) {
			if (projected[from] <= targets[from]) break;
			const to = quotaColors
				.filter((c) => projected[c] < targets[c])
				.sort((a, b) => targets[b] - projected[b] - (targets[a] - projected[a]))[0];
			if (!to) break;
			moves.push({ steamId: player.steamId, from: teams[from], to: teams[to] });
			projected[from]--;
			projected[to]++;
		}
	}
	return {
		counts,
		targets,
		moves,
		reason: moves.length
			? ''
			: quotaColors.some((c) => counts[c] !== targets[c])
				? '等待之前调队确认或冷却结束。'
				: '人数已符合配额比例。'
	};
}
