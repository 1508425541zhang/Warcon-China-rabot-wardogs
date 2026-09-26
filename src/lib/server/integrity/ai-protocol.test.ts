import { expect, test } from 'bun:test';
import { apiBase, numericChecks, reviewOutput } from './ai-protocol';
test('API base keeps provider prefixes and rejects secret-bearing or insecure URLs', () => {
	expect(apiBase('https://example.com/compatible/v1/')).toBe('https://example.com/compatible/v1');
	for (const url of [
		'http://example.com/v1',
		'https://u:p@example.com/v1',
		'https://example.com/v1?key=x',
		'https://example.com/v1/chat/completions'
	])
		expect(() => apiBase(url)).toThrow();
});
test('missing counts stay unknown; mismatch is exposed without changing risk', () => {
	expect(numericChecks({}).kpm180.recomputed).toBeNull();
	expect(numericChecks({ infantryKills: 9, kpm180: 3 }).kpm180.matches).toBe(true);
	expect(numericChecks({ infantryKills: 9, kpm180: 4 }).kpm180.matches).toBe(false);
});
test('incomplete or invented verdict is not a valid model review', () => {
	expect(
		reviewOutput.safeParse({
			verdict: '永久封禁',
			summary: 'x',
			reasons: [],
			contradictions: [],
			missingEvidence: []
		}).success
	).toBe(false);
	expect(reviewOutput.safeParse({ verdict: '建议复核' }).success).toBe(false);
});
