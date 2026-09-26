export function factionLockDecision(input: {
	from: string | null;
	to: string | null;
	teams: string[];
	authorized: boolean;
	transition: boolean;
	full: boolean | null;
	leading: boolean | null;
}) {
	if (
		!input.from ||
		!input.to ||
		!input.teams.includes(input.from) ||
		!input.teams.includes(input.to)
	)
		return 'INITIAL_OR_UNASSIGNED';
	if (input.from === input.to) return 'UNCHANGED';
	if (input.authorized) return 'AUTHORIZED_MOVE';
	if (input.transition) return 'ROUND_GRACE';
	if (input.full === true)
		return input.leading === true
			? 'WARN_KICK'
			: input.leading === false
				? 'FULL_NOT_LEADING'
				: 'UNKNOWN_SCORE';
	return 'RESTORE';
}
export function originalFactionLeading(
	scores: readonly { name: string; score: number }[],
	faction: string
): boolean | null {
	const own = scores.find((s) => s.name === faction);
	if (!own || scores.length < 2 || scores.some((s) => !Number.isFinite(s.score))) return null;
	return scores.filter((s) => s.name !== faction).every((s) => own.score > s.score);
}
