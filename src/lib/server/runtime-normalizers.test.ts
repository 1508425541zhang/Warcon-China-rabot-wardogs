import { describe, expect, test } from 'bun:test';
import {
	normalizeScores,
	normalizeStatus,
	normalizeWebhookEvents,
	webhookScopeAllows
} from './runtime-normalizers';

describe('historic JSON normalization', () => {
	test('scores ignore null and non-numeric elements', () => {
		expect(
			normalizeScores([null, {}, { name: 'Blue', score: '12' }, { name: 'Red', score: 'NaN' }])
		).toEqual([{ name: 'Blue', score: 12 }]);
		expect(normalizeScores({ score: 1 })).toBeNull();
	});
	test('cold live status normalizes malformed scores and rejects missing identity', () => {
		expect(
			normalizeStatus({ serverName: 'S', map: 'Kavkazi', scores: [null], playerCount: 3 })?.scores
		).toEqual([]);
		expect(normalizeStatus({ scores: [null] })).toBeNull();
	});
	test('malformed webhook arrays fail closed', () => {
		expect(normalizeWebhookEvents({ events: ['players'] })).toEqual([]);
		expect(webhookScopeAllows({ server: 's1' }, 's1')).toBe(false);
		expect(webhookScopeAllows(null, 's1')).toBe(true);
		expect(webhookScopeAllows(['s2'], 's1')).toBe(false);
	});
});
