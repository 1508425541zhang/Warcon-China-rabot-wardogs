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
	lastKickClock: number | null
) {
	if (
		!Number.isFinite(eventClock) ||
		!Number.isFinite(nowClock) ||
		eventClock > nowClock + 5 ||
		eventClock < nowClock - 60
	)
		return null;
	if (lastKickClock !== null && eventClock < lastKickClock + 60) return null;
	return 'kick';
}

/** Live builds may omit matchSeconds. Only fresh, same-round feed data may supply it. */
export function restrictionClock(
	statusClock: number | null,
	statusAt: number,
	now: number,
	anchor: { eventClock: number; receivedAt: number } | null
) {
	if (!Number.isFinite(now) || !Number.isFinite(statusAt) || now - statusAt > 30000) return null;
	if (statusClock !== null)
		return Number.isFinite(statusClock) && statusClock >= 0
			? statusClock + Math.max(0, now - statusAt) / 1000
			: null;
	if (
		!anchor ||
		!Number.isFinite(anchor.eventClock) ||
		anchor.eventClock < 0 ||
		!Number.isFinite(anchor.receivedAt) ||
		now - anchor.receivedAt > 30000 ||
		anchor.receivedAt - now > 5000
	)
		return null;
	return anchor.eventClock + Math.max(0, now - anchor.receivedAt) / 1000;
}
