import { describe, expect, test } from 'bun:test';
import { fmtUptime, restartWindow } from './uptime';

const T0 = Date.parse('2026-09-14T00:00:00Z');
const H = 3600_000;

describe('restartWindow', () => {
	test('unknown start time is null', () => {
		expect(restartWindow(null, 12, T0)).toBeNull();
		expect(restartWindow('', 12, T0)).toBeNull();
		expect(restartWindow('not a date', 12, T0)).toBeNull();
	});
	test('before the threshold: a window that opens later', () => {
		const w = restartWindow(new Date(T0 - 9 * H).toISOString(), 12, T0)!;
		expect(w.upMs).toBe(9 * H);
		expect(w.due).toBe(false);
		expect(w.untilDueMs).toBe(3 * H);
	});
	test('past the threshold: due, restarting after this round', () => {
		const w = restartWindow(new Date(T0 - 12.5 * H).toISOString(), 12, T0)!;
		expect(w.due).toBe(true);
		expect(w.untilDueMs).toBeNull();
		expect(w.upMs).toBe(12.5 * H);
	});
	test('exactly on the threshold counts as due', () => {
		expect(restartWindow(new Date(T0 - 12 * H).toISOString(), 12, T0)!.due).toBe(true);
	});
	test('no scheduled restart: uptime only', () => {
		const w = restartWindow(new Date(T0 - 30 * H).toISOString(), 0, T0)!;
		expect(w.due).toBe(false);
		expect(w.untilDueMs).toBeNull();
		expect(w.upMs).toBe(30 * H);
	});
	test('a start time in the future (clock skew) reads as just started', () => {
		expect(restartWindow(new Date(T0 + 5000).toISOString(), 12, T0)!.upMs).toBe(0);
	});
});

describe('fmtUptime', () => {
	test('units', () => {
		expect(fmtUptime(0)).toBe('<1m');
		expect(fmtUptime(59_000)).toBe('<1m');
		expect(fmtUptime(12 * 60_000)).toBe('12m');
		expect(fmtUptime(9 * H + 12 * 60_000)).toBe('9h 12m');
		expect(fmtUptime(9 * H)).toBe('9h');
		expect(fmtUptime(26 * H + 30 * 60_000)).toBe('1d 2h');
		expect(fmtUptime(48 * H)).toBe('2d');
	});
});
