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

	test('a failure never says where RCON listens: the message reaches every viewer and the Discord card', async () => {
		const failure = await gameRequest(
			{ host: 'not-a-real-host.invalid', port: 9, scheme: 'http', addresses: ['127.0.0.1'] },
			{ method: 'GET', path: '/v1/status', timeoutMs: 2000 }
		).catch((err: Error) => err);
		expect(failure).toBeInstanceOf(Error);
		const message = (failure as Error).message;
		expect(message).toStartWith('Could not reach the game server');
		expect(message).not.toContain('not-a-real-host');
		expect(message).not.toContain('127.0.0.1');
		expect(message).not.toContain(':9');
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

	describe('a command that reached the game is never sent again', () => {
		// A second game server on the same port of the other loopback address. `first` takes the
		// request on [::1] and breaks off; the HTTP server on 127.0.0.1 counts what it is sent.
		let posts = 0;
		const second = Bun.serve({
			port: 0,
			hostname: '127.0.0.1',
			fetch() {
				posts++;
				return new Response('{}');
			}
		});
		const target = {
			host: 'not-a-real-host.invalid',
			port: second.port as number,
			scheme: 'http' as const,
			addresses: ['::1', '127.0.0.1']
		};
		afterAll(() => second.stop(true));

		async function firstAnswers(
			answer: string
		): Promise<{ taken: () => number; stop: () => void }> {
			let taken = 0;
			const first = Bun.listen({
				hostname: '::1',
				port: target.port,
				socket: {
					data(socket) {
						taken++;
						if (answer) socket.write(answer);
						socket.end();
					}
				}
			});
			return { taken: () => taken, stop: () => first.stop(true) };
		}

		test('the answer breaks off after its headers', async () => {
			posts = 0;
			const first = await firstAnswers('HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\npart');
			try {
				await expect(
					gameRequest(target, { method: 'POST', path: '/v1/match/end', body: '{}' })
				).rejects.toThrow('Could not reach the game server');
				expect(first.taken()).toBe(1);
				expect(posts).toBe(0);
			} finally {
				first.stop();
			}
		});

		test('the connection drops with the request sent and nothing answered', async () => {
			posts = 0;
			const first = await firstAnswers('');
			try {
				await expect(
					gameRequest(target, { method: 'POST', path: '/v1/match/end', body: '{}' })
				).rejects.toThrow('Could not reach the game server');
				expect(first.taken()).toBe(1);
				expect(posts).toBe(0);
			} finally {
				first.stop();
			}
		});
	});
});
