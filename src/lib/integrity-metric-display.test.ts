import { expect, test } from 'bun:test';
import { integrityMetricDisplay } from './integrity-metric-display';

test('partial data shows a confirmed lower bound, never a false complete zero', () => {
	expect(integrityMetricDisplay(2, true, false)).toBe('≥2.00');
	expect(integrityMetricDisplay(0, true, false)).toBe('—');
	expect(integrityMetricDisplay(0, true, true)).toBe('0.00');
	expect(integrityMetricDisplay(2, false, true)).toBe('—');
});
