// Is this request for the JSON API? hooks.server.ts hangs the bearer key, the CSRF header rule
// and the JSON form of its refusals on the answer.

/**
 * SvelteKit routes on the decoded path, so `/%61pi/servers` reaches the handler of `/api/servers`
 * while the path as sent does not start with `/api/`. The route it matched is asked as well as
 * the path: either one saying "API" makes it one, and every rule for the API then applies.
 */
export const isApiRequest = (pathname: string, routeId: string | null): boolean =>
	pathname.startsWith('/api/') || (routeId ?? '').startsWith('/api/');
