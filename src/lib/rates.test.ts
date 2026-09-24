import { describe, expect, test } from 'bun:test';
import { fmtRate, meanOf, perSecond } from './rates';
import { fmtBytes, fmtCompact } from './format';

describe('overview rates', () => {
	test('a rate needs two readings and reads per second between them', () => {
		expect(perSecond(undefined, 50, undefined, 5000)).toBeNull();
		expect(perSecond(20, 50, 0, 5000)).toBe(6);
		expect(perSecond(20, 20, 0, 5000)).toBe(0);
	});
	test('a counter that went backwards (a restart) gives no rate for that interval', () => {
		expect(perSecond(500, 3, 0, 5000)).toBeNull();
		expect(perSecond(1, 2, 5000, 5000)).toBeNull();
	});
	test('the mean of a histogram between readings is its sum over its count', () => {
		expect(meanOf(10, 13, 100, 120)).toBeCloseTo(0.15);
		expect(meanOf(10, 10, 100, 100)).toBeNull();
		expect(meanOf(undefined, 10, undefined, 100)).toBeNull();
	});
	test('rates and sizes read at a glance', () => {
		expect(fmtRate(null)).toBe('…');
		expect(fmtRate(6.84)).toBe('6.8/s');
		expect(fmtRate(96.4)).toBe('96/s');
		expect(fmtRate(4.2, ' / min')).toBe('4.2 / min');
		expect(fmtBytes(6_200_000_000)).toBe('6.2 GB');
		expect(fmtBytes(12_900_000_000)).toBe('12.9 GB');
		expect(fmtBytes(640_000_000)).toBe('640 MB');
		expect(fmtBytes(812)).toBe('812 B');
		expect(fmtCompact(12_900_000)).toBe('12.9 M');
		expect(fmtCompact(318_400)).toBe('318 K');
		expect(fmtCompact(964)).toBe('964');
	});
});
