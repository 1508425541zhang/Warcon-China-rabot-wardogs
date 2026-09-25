import { afterEach, describe, expect, test } from 'bun:test';
import { resetRates } from './ratelimit';
import { resetFeedAuthLimits, resolveLimitedFeedToken } from './feed-auth-limit';

afterEach(() => {
	resetRates();
	resetFeedAuthLimits();
});

describe('feed token preflight', () => {
	test('random invalid tokens stop invoking the database lookup once the invalid budget is spent', async () => {
		let lookups = 0;
		const lookup = async (_token: string) => {
			lookups++;
			return null;
		};
		for (let i = 0; i < 100; i++)
			expect(await resolveLimitedFeedToken('192.0.2.10', `wkf_${i}`, lookup)).toBeNull();
		for (let i = 100; i < 130; i++)
			await expect(resolveLimitedFeedToken('192.0.2.10', `wkf_${i}`, lookup)).rejects.toMatchObject(
				{ status: 429 }
			);
		expect(lookups).toBe(100);
	});
	test('many valid feed posts from servers behind the same address remain allowed', async () => {
		let lookups = 0;
		const lookup = async (_token: string) => {
			lookups++;
			return 'server';
		};
		for (let i = 0; i < 1200; i++)
			expect(await resolveLimitedFeedToken('192.0.2.11', `valid-${i}`, lookup)).toBe('server');
		expect(lookups).toBe(1200);
	});
});
