import { describe, expect, test } from 'bun:test';
import { feedUrl } from './feed';
import { POST as ingest } from '../../routes/api/ingest/events/+server';
import { POST as legacy } from '../../routes/api/feed/events/api/ingest/events/+server';

describe('feed endpoint', () => {
	test('the Url written to the config is the origin alone: the game appends /api/ingest/events', () => {
		expect(feedUrl({ ORIGIN: 'https://console.example' })).toBe('https://console.example');
	});
	test('the path an older config produces is served by the same handler', () => {
		expect(legacy).toBe(ingest);
	});
});
