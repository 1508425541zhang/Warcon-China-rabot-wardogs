// Rates the Admin overview derives from two readings of a cumulative counter, five seconds apart.
// A counter that went backwards means the process restarted between readings: no rate for that
// interval rather than a negative one.

/** Events per second between two readings; null before the second reading or across a restart. */
export function perSecond(
	prev: number | undefined,
	next: number,
	prevAt: number | undefined,
	nextAt: number
): number | null {
	if (prev === undefined || prevAt === undefined) return null;
	const dt = (nextAt - prevAt) / 1000;
	if (dt <= 0 || next < prev) return null;
	return (next - prev) / dt;
}

/** Mean of a histogram between two readings (its sum over its count), null when nothing happened. */
export function meanOf(
	sumPrev: number | undefined,
	sumNext: number,
	countPrev: number | undefined,
	countNext: number
): number | null {
	if (sumPrev === undefined || countPrev === undefined) return null;
	const n = countNext - countPrev;
	if (n <= 0 || sumNext < sumPrev) return null;
	return (sumNext - sumPrev) / n;
}

/** 6.8 for kills a second, 96 for observations, 0.3 for a trickle: one decimal below ten. */
export const fmtRate = (v: number | null, unit = '/s'): string =>
	v === null ? '…' : `${v < 10 ? v.toFixed(1) : Math.round(v)}${unit}`;
