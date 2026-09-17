import { describe, expect, mock, test } from 'bun:test';

// The route reaches env.ts, which imports SvelteKit's env alias; outside the app that alias does
// not resolve, so it is stubbed before the route is loaded.
mock.module('$env/dynamic/private', () => ({ env: process.env }));
const { feedUrl } = await import('./feed');
const { POST: ingest } = await import('../../routes/api/ingest/events/+server');
const { POST: legacy } = await import('../../routes/api/feed/events/api/ingest/events/+server');

describe('feed endpoint', () => {
	test('the Url written to the config is the origin alone: the game appends /api/ingest/events', () => {
		expect(feedUrl({ ORIGIN: 'https://console.example' })).toBe('https://console.example');
	});
	test('the path an older config produces is served by the same handler', () => {
		expect(legacy).toBe(ingest);
	});
});
