import { describe, expect, test } from 'bun:test';
import { fmtAgo, fmtSpan, mapId } from './format';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

describe('fmtSpan', () => {
	test('picks the coarsest unit that still reads', () => {
		expect(fmtSpan(0)).toBe('0 秒');
		expect(fmtSpan(40_000)).toBe('40 秒');
		expect(fmtSpan(12 * MIN)).toBe('12 分钟');
		expect(fmtSpan(90 * MIN)).toBe('2 小时');
		expect(fmtSpan(35 * HOUR)).toBe('35 小时');
		expect(fmtSpan(DAY)).toBe('24 小时');
		expect(fmtSpan(36 * HOUR)).toBe('2 天');
		expect(fmtSpan(6 * DAY)).toBe('6 天');
	});
});

describe('fmtAgo', () => {
	const now = Date.parse('2026-09-17T12:00:00Z');
	test('reads as a status line', () => {
		expect(fmtAgo(now - 10_000, now)).toBe('刚刚');
		expect(fmtAgo(now - 2 * MIN, now)).toBe('2 分钟前');
		expect(fmtAgo(now - 4 * HOUR, now)).toBe('4 小时前');
		expect(fmtAgo(now - 3 * DAY, now)).toBe('3 天前');
	});
	test('takes ISO strings and never goes negative', () => {
		expect(fmtAgo(new Date(now - 5 * MIN).toISOString(), now)).toBe('5 分钟前');
		expect(fmtAgo(now + 10 * MIN, now)).toBe('刚刚');
	});
	test('falls back to the date past a month', () => {
		expect(fmtAgo(now - 47 * DAY, now)).not.toContain('ago');
		expect(fmtAgo('not a date', now)).toBe('not a date');
	});
});

test('mapId: the name players know and the catalog id are one map', () => {
	expect(mapId('NorthAmerica')).toBe('NorthAmerica');
	expect(mapId('Zestafona')).toBe('NorthAmerica');
	expect(mapId('bakurani')).toBe('Kavkazi');
	expect(mapId('SomeNewMap')).toBe('SomeNewMap');
	expect(mapId('')).toBe('');
});
