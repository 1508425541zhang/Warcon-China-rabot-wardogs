/** Empirical frequency polygon: each eligible player contributes once, never a fitted bell curve. */
export function weaponDistanceDistribution(averages: readonly number[], current: number) {
	const values = averages.filter((v) => Number.isFinite(v) && v > 0);
	const upper = Math.max(1, ...values, Number.isFinite(current) ? current : 0) * 1.05;
	const binCount = 12;
	const step = upper / binCount;
	const counts = Array<number>(binCount).fill(0);
	for (const value of values) counts[Math.min(binCount - 1, Math.floor(value / step))]++;
	return {
		upper,
		step,
		players: values.length,
		peak: Math.max(1, ...counts),
		bins: counts.map((count, i) => ({
			from: i * step,
			to: (i + 1) * step,
			center: (i + 0.5) * step,
			count
		}))
	};
}
