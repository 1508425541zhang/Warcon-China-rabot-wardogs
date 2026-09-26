export const RESTRICTION_GROUPS = [
	{ id: 'items', label: '全部单兵武器／物品（含枪支、手雷、爆炸物、近战）' },
	{ id: 'vehicles', label: '全部载具本体与载具武器' },
	{ id: 'buildables', label: '全部建筑／固定设施来源' }
] as const;
export function restrictedCause(
	cause: string | null,
	causes: readonly string[],
	groups: readonly string[]
) {
	if (!cause) return false;
	if (causes.includes(cause)) return true;
	return (
		(groups.includes('items') && /^Id\.Item\./i.test(cause)) ||
		(groups.includes('vehicles') && /^(Vehicle\.|Id\.Vehicle\.)/i.test(cause)) ||
		(groups.includes('buildables') && /^Id\.Buildable\./i.test(cause))
	);
}
export function restrictionStage(
	eventClock: number,
	nowClock: number,
	warning: { clock: number } | undefined,
	lastKickClock: number | null
) {
	if (
		!Number.isFinite(eventClock) ||
		!Number.isFinite(nowClock) ||
		eventClock > nowClock + 5 ||
		eventClock < nowClock - 60
	)
		return null;
	if (!warning) return 'warn';
	// Only a NEW kill after the warning grace can escalate. One grenade's multi-kill cannot.
	if (eventClock < warning.clock + 8) return null;
	if (lastKickClock !== null && eventClock < lastKickClock + 60) return null;
	return 'kick';
}
