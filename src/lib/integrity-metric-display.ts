/** Incomplete evidence is a lower bound, never a complete zero or an enforcement input. */
export function integrityMetricDisplay(
	value: number,
	available: boolean,
	reliable: boolean,
	digits = 2
): string {
	if (!available || (!reliable && value === 0)) return '—';
	return `${reliable ? '' : '≥'}${value.toFixed(digits)}`;
}
