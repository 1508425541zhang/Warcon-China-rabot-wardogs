import { expect, test } from 'bun:test';
import { call } from './gateway-remote';
import type { Env } from './env';

test('a worker that cannot be reached is said plainly: the runtime quotes the relay URL in some failures', async () => {
	const notHttp = Bun.listen({
		hostname: '127.0.0.1',
		port: 0,
		socket: {
			data(socket) {
				socket.end('not http at all\r\n\r\n');
			}
		}
	});
	const env = {
		RELAY_URL: `http://127.0.0.1:${notHttp.port}`,
		RELAY_SECRET: 'x'.repeat(16)
	} as Env;
	const logged = console.error;
	console.error = () => {};
	try {
		const failure = await call(env, '/health', undefined, 'GET', 2000).catch((e: Error) => e);
		expect((failure as Error).message).toBe('The worker is not reachable.');
	} finally {
		console.error = logged;
		notHttp.stop(true);
	}
});
