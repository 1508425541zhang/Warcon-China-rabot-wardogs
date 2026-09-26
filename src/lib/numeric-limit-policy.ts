export type NumericLimits = {
	enabled: boolean;
	kpm: number | null;
	kd: number | null;
	cash: number | null;
	windowSeconds: number;
	minKills: number;
};
export const defaultNumericLimits: NumericLimits = {
	enabled: false,
	kpm: null,
	kd: null,
	cash: null,
	windowSeconds: 180,
	minKills: 10
};
export function validNumericLimits(v: any): v is NumericLimits {
	return (
		v &&
		typeof v.enabled === 'boolean' &&
		Number.isInteger(v.windowSeconds) &&
		v.windowSeconds >= 60 &&
		v.windowSeconds <= 900 &&
		Number.isInteger(v.minKills) &&
		v.minKills >= 1 &&
		v.minKills <= 1000 &&
		['kpm', 'kd', 'cash'].every(
			(k) =>
				v[k] === null ||
				(typeof v[k] === 'number' && Number.isFinite(v[k]) && v[k] > 0 && v[k] <= 100000000)
		) &&
		(!v.enabled || [v.kpm, v.kd, v.cash].some((x) => x !== null))
	);
}
export type LimitObservation = { at: number; kills: number; deaths: number; cash: number };
/** Cash is observed net growth, not gross earnings. Missing/reset counters never become zero. */
export function numericBreaches(rule: NumericLimits, points: LimitObservation[]) {
	if (points.length < 2) return [];
	const first = points[0],
		last = points[points.length - 1],
		seconds = (last.at - first.at) / 1000;
	if (seconds < rule.windowSeconds || seconds > rule.windowSeconds + 60) return [];
	if (
		points.some(
			(p, i) =>
				![p.at, p.kills, p.deaths, p.cash].every(Number.isFinite) ||
				p.kills < 0 ||
				p.deaths < 0 ||
				(i > 0 &&
					(p.at <= points[i - 1].at ||
						p.at - points[i - 1].at > 65000 ||
						p.kills < points[i - 1].kills ||
						p.deaths < points[i - 1].deaths))
		)
	)
		return [];
	const values = {
		kpm: ((last.kills - first.kills) * 60) / seconds,
		kd: last.kills / Math.max(last.deaths, 1),
		cash: ((last.cash - first.cash) * 60) / seconds
	};
	return (['kpm', 'kd', 'cash'] as const).flatMap((metric) =>
		rule[metric] !== null &&
		values[metric] > rule[metric]! &&
		(metric !== 'kd' || last.kills >= rule.minKills)
			? [{ metric, value: values[metric], limit: rule[metric]! }]
			: []
	);
}
