// Loaded before every test file (bunfig.toml). The shared server code imports three SvelteKit
// virtual modules that only exist inside Vite; outside it they are stubbed the way
// scripts/build-worker.ts stubs them for the worker bundle.
import { afterAll, mock } from 'bun:test';

mock.module('$env/dynamic/private', () => ({ env: process.env }));
mock.module('$app/environment', () => ({ building: false, dev: false, browser: false }));
// auth.ts asks for the request event to set cookies; no test signs anyone in through Better Auth.
mock.module('$app/server', () => ({
	getRequestEvent: () => {
		throw new Error('No request event in tests.');
	}
}));

// Hooks in a preload run once for the whole run, not once per file.
afterAll(async () => {
	const { dropTestDb } = await import('./db');
	await dropTestDb();
});
