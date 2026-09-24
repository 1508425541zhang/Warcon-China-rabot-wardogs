import { test, expect } from 'bun:test';
import { classifyGameError, etagOf, parseRetryAfterMs } from './rcon';

// Shapes captured from live build ++Wardogs+Live-CL-499480 on 2026-09-11.
test('a route the build does not serve becomes no_route with the route in the message', () => {
	const e = classifyGameError('POST', '/v1/reserved-slots', 404, 'Not Found', {
		error: { code: 'not_found', message: 'No such endpoint.' }
	});
	expect(e.code).toBe('no_route');
	expect(e.status).toBe(404);
	expect(e.message).toBe('This server build does not serve POST /v1/reserved-slots.');
});

test('a missing ban keeps its own code', () => {
	const e = classifyGameError('DELETE', '/v1/bans/1', 404, 'Not Found', {
		error: { code: 'ban_not_found', message: 'Error: SteamId 1 is not currently banned.' }
	});
	expect(e.code).toBe('ban_not_found');
	expect(e.message).toContain('not currently banned');
});

test('a wrong method names the route', () => {
	const e = classifyGameError('PUT', '/v1/sponsor', 405, '', {
		error: { code: 'method_not_allowed', message: 'PUT is not supported on this endpoint.' }
	});
	expect(e.code).toBe('method_not_allowed');
	expect(e.message).toContain('PUT /v1/sponsor');
});

test('other failures pass the server message through', () => {
	const e = classifyGameError('GET', '/v1/status', 401, 'Unauthorized', {
		error: { code: 'unauthorized', message: 'Bad token.' }
	});
	expect(e.message).toBe('Bad token.');
	expect(classifyGameError('GET', '/v1/x', 500, 'Boom', null).message).toBe(
		'Server answered 500 Boom.'
	);
});

// Live build CL-501228 (2026-09-14) exposes Retry-After and an ETag on the config document.
test('a 429 becomes rate_limited with the Retry-After the listener gave', () => {
	const e = classifyGameError(
		'GET',
		'/v1/players',
		429,
		'Too Many Requests',
		{ error: { code: 'rate_limited', message: 'Too many requests.' } },
		{ 'retry-after': '7' }
	);
	expect(e.code).toBe('rate_limited');
	expect(e.status).toBe(429);
	expect(e.retryAfterMs).toBe(7000);
	expect(e.message).toContain('retry in 7 s');
	// No header, empty body: still classified, with the default wait.
	const bare = classifyGameError('GET', '/v1/players', 429, '', null, {});
	expect(bare.code).toBe('rate_limited');
	expect(bare.retryAfterMs).toBe(5000);
});

test('Retry-After: seconds, an HTTP date, garbage, and the 1–60 s clamp', () => {
	const now = Date.parse('2026-09-14T10:00:00Z');
	expect(parseRetryAfterMs('7', now)).toBe(7000);
	expect(parseRetryAfterMs('Mon, 14 Sep 2026 10:00:12 GMT', now)).toBe(12_000);
	expect(parseRetryAfterMs(undefined, now)).toBe(5000);
	expect(parseRetryAfterMs('soon', now)).toBe(5000);
	expect(parseRetryAfterMs('0', now)).toBe(1000);
	expect(parseRetryAfterMs('900', now)).toBe(60_000);
	expect(parseRetryAfterMs('Mon, 14 Sep 2026 09:00:00 GMT', now)).toBe(1000);
});

test('the ETag is the revision, unquoted', () => {
	expect(etagOf({ etag: '"d31a26d94a9c"' })).toBe('d31a26d94a9c');
	expect(etagOf({ etag: 'W/"abc"' })).toBe('abc');
	expect(etagOf({})).toBe('');
});
