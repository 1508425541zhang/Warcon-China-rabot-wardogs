import { expect, test } from 'bun:test';
import { isApiRequest } from './api-path';

test('a path SvelteKit decodes into an API route is an API request, however it was written', () => {
	expect(isApiRequest('/api/servers', '/api/servers')).toBe(true);
	expect(isApiRequest('/%61pi/orgs/x/lists/sync', '/api/orgs/[id]/lists/sync')).toBe(true);
	expect(isApiRequest('/%41PI/servers', null)).toBe(false);
	// Better Auth's own paths match no SvelteKit route and are told by the path alone.
	expect(isApiRequest('/api/auth/callback/discord', null)).toBe(true);
	expect(isApiRequest('/server/x/bans', '/(app)/server/[id]/bans')).toBe(false);
});
