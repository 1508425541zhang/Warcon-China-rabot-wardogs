import { describe, expect, test } from 'bun:test';
import { tickBand, tickReward } from './tick';

describe('tick bands', () => {
	test('each band starts at its own boundary', () => {
		expect(tickBand(30)?.label).toBe('30s+');
		expect(tickBand(29)?.label).toBe('27–29s');
		expect(tickBand(27)?.label).toBe('27–29s');
		expect(tickBand(26)?.label).toBe('24–26s');
		expect(tickBand(24)?.label).toBe('24–26s');
		expect(tickBand(21)?.label).toBe('21–23s');
		expect(tickBand(20)?.label).toBe('0–20s');
		expect(tickBand(18)?.label).toBe('0–20s');
	});

	test('the rewards the official console shows for the TLR default (24 s)', () => {
		expect(tickReward(24)).toEqual({
			control: 160,
			hot: 320,
			band: { from: 24, multiplier: 0.8, label: '24–26s' }
		});
	});

	test('full rate at 30 s and above, floor band below the range', () => {
		expect(tickReward(30)).toMatchObject({ control: 200, hot: 400 });
		expect(tickReward(45)).toMatchObject({ control: 200, hot: 400 });
		expect(tickReward(-1)).toMatchObject({ control: 120, hot: 240 });
	});

	test('unknown cadence gives no reward', () => {
		expect(tickReward(null)).toBeNull();
		expect(tickReward(undefined)).toBeNull();
		expect(tickReward(NaN)).toBeNull();
	});
});
