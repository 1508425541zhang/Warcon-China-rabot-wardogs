import { beforeEach, describe, expect, test } from 'bun:test';
import {
	metricsResponse,
	observations,
	registerCollector,
	renderMetrics,
	resetMetrics,
	routeLabel,
	serversByTier
} from './metrics';
import { assertRate, resetRates } from './ratelimit';

beforeEach(() => {
	resetMetrics();
	resetRates();
});

describe('metrics', () => {
	test('counters render in the exposition with their labels, beside the process figures', async () => {
		observations.inc({ outcome: 'ok' }, 3);
		observations.inc({ outcome: 'failed' });
		const text = await renderMetrics();
		expect(text).toContain('warcon_observations_total{outcome="ok"} 3');
		expect(text).toContain('warcon_observations_total{outcome="failed"} 1');
		expect(text).toContain('process_resident_memory_bytes');
	});

	test('collectors run before every scrape, and one that fails leaves the others standing', async () => {
		registerCollector(() => {
			serversByTier.set({ tier: 'hot' }, 7);
		});
		registerCollector(async () => {
			throw new Error('database away');
		});
		expect(await renderMetrics()).toContain('warcon_servers{tier="hot"} 7');
	});

	test('the endpoint is off without a token, refuses a wrong bearer and serves the right one', async () => {
		const req = (auth?: string) =>
			new Request('http://panel/metrics', { headers: auth ? { authorization: auth } : {} });
		expect((await metricsResponse(req('Bearer s3cret'), undefined)).status).toBe(404);
		expect((await metricsResponse(req(), 's3cret')).status).toBe(401);
		expect((await metricsResponse(req('Bearer wrong'), 's3cret')).status).toBe(401);
		const ok = await metricsResponse(req('Bearer s3cret'), 's3cret');
		expect(ok.status).toBe(200);
		expect(ok.headers.get('content-type')).toContain('text/plain');
		expect(await ok.text()).toContain('# TYPE warcon_http_requests_total counter');
	});

	test('a refused request counts under the limit that fired', async () => {
		assertRate('feed:server-1', 1, 60_000);
		expect(() => assertRate('feed:server-1', 1, 60_000)).toThrow();
		expect(await renderMetrics()).toContain('warcon_rate_limited_total{scope="feed"} 1');
	});

	test('unmatched requests share one route label', () => {
		expect(routeLabel(null)).toBe('(unmatched)');
		expect(routeLabel('/api/servers/[id]/analytics')).toBe('/api/servers/[id]/analytics');
	});
});
