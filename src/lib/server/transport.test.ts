import { afterAll, describe, expect, test } from 'bun:test';
import { gameRequest } from './transport';

// The point of pinning: Warcon resolves and validates a hostname, then connects to that exact
// address, so a name that passed the host-policy check cannot rebind to an internal address before
// the socket opens. These tests run entirely on loopback: the target host is a name that does not
// resolve, so a request only succeeds if it used the pinned address instead of resolving the name.
const server = Bun.serve({
	port: 0,
	hostname: '127.0.0.1',
	fetch(req) {
		return new Response(JSON.stringify({ host: req.headers.get('host'), url: req.url }), {
			headers: { 'content-type': 'application/json' }
		});
	}
});
const PORT = server.port as number;
afterAll(() => server.stop(true));

describe('gameRequest address pinning', () => {
	test('connects to the pinned address, not by resolving the host', async () => {
		const res = await gameRequest(
			{ host: 'not-a-real-host.invalid', port: PORT, scheme: 'http', addresses: ['127.0.0.1'] },
			{ method: 'GET', path: '/v1/status' }
		);
		expect(res.status).toBe(200);
		const body = JSON.parse(res.text);
		// The hostname still travels as the Host header, so virtual hosts and logs are unchanged.
		expect(body.host).toBe(`not-a-real-host.invalid:${PORT}`);
	});

	test('without a pinned address the unresolvable host cannot connect (the test is meaningful)', async () => {
		await expect(
			gameRequest(
				{ host: 'not-a-real-host.invalid', port: PORT, scheme: 'http' },
				{ method: 'GET', path: '/v1/status' }
			)
		).rejects.toThrow();
	});

	test('falls back to the next validated address when the first is unreachable', async () => {
		// The server binds 127.0.0.1 only, so [::1] refuses at once; the request must still land on
		// the second validated address. This is the happy-eyeballs resilience the old by-hostname
		// fetch had, kept while every attempt stays on an approved address.
		const res = await gameRequest(
			{
				host: 'not-a-real-host.invalid',
				port: PORT,
				scheme: 'http',
				addresses: ['::1', '127.0.0.1']
			},
			{ method: 'GET', path: '/v1/status' }
		);
		expect(res.status).toBe(200);
		expect(JSON.parse(res.text).host).toBe(`not-a-real-host.invalid:${PORT}`);
	});

	test('a bare-IP target pins to itself and sends the normal Host', async () => {
		const res = await gameRequest(
			{ host: '127.0.0.1', port: PORT, scheme: 'http', addresses: ['127.0.0.1'] },
			{ method: 'GET', path: '/v1/status' }
		);
		expect(res.status).toBe(200);
		expect(JSON.parse(res.text).host).toBe(`127.0.0.1:${PORT}`);
	});
});
