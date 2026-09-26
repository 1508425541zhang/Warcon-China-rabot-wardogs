import type { Player, Status } from './types';

export type BalancePlayer = Player & { kpm: number; kd: number };
export type BalancePlan = {
	strong: string;
	weak: string;
	pairs: { strong: BalancePlayer; weak: BalancePlayer }[];
};
/** Exactly three pairs. Deterministic ties; no movement unless every pair improves the weak side. */
export function planSkillBalance(
	status: Status,
	players: BalancePlayer[],
	leadPoints: number
): BalancePlan | null {
	if (!Number.isFinite(leadPoints) || leadPoints < 40) return null;
	const scores = [...status.scores].sort((a, b) => b.score - a.score);
	if (
		scores.length !== 3 ||
		new Set(scores.map((s) => s.name)).size !== scores.length ||
		scores.some((s) => !s.name || !Number.isFinite(s.score) || s.score < 0)
	)
		return null;
	const strong = scores[0],
		weak = scores.at(-1)!;
	if (
		strong.score <= 0 ||
		strong.score === scores[1].score ||
		weak.score === scores.at(-2)!.score ||
		strong.score - scores[1].score <= leadPoints
	)
		return null;
	const rank = (a: BalancePlayer, b: BalancePlayer) =>
		b.kpm - a.kpm || b.kd - a.kd || a.steamId.localeCompare(b.steamId);
	const eligible = players.filter(
		(p) =>
			/^\d{17}$/.test(p.steamId) &&
			Number.isFinite(p.kpm) &&
			p.kpm >= 0 &&
			Number.isFinite(p.kd) &&
			p.kd >= 0
	);
	if (new Set(eligible.map((p) => p.steamId)).size !== eligible.length) return null;
	const top = eligible
		.filter((p) => p.faction === strong.name)
		.sort(rank)
		.slice(0, 3);
	const bottom = eligible
		.filter((p) => p.faction === weak.name)
		.sort((a, b) => a.kpm - b.kpm || a.kd - b.kd || a.steamId.localeCompare(b.steamId))
		.slice(0, 3);
	if (
		top.length !== 3 ||
		bottom.length !== 3 ||
		top.some((p, i) => p.kpm < bottom[i].kpm || (p.kpm === bottom[i].kpm && p.kd <= bottom[i].kd))
	)
		return null;
	return {
		strong: strong.name,
		weak: weak.name,
		pairs: top.map((p, i) => ({ strong: p, weak: bottom[i] }))
	};
}
