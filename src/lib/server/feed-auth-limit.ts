import { ApiError } from './http';
import { assertRate } from './ratelimit';

const WINDOW_MS = 60_000;
// A coarse ceiling allows many busy servers behind one NAT; verified servers also have their own cap.
const IP_POSTS_PER_MINUTE = 6000;
const INVALID_LOOKUPS_PER_MINUTE = 100;
const invalid = new Map<string, { count: number; until: number }>();

/** Reject abusive invalid-token traffic before it can repeatedly query Postgres. */
export async function resolveLimitedFeedToken(
	ip: string,
	token: string | null,
	lookup: (token: string) => Promise<string | null>
): Promise<string | null> {
	assertRate(`feed-ip:${ip}`, IP_POSTS_PER_MINUTE, WINDOW_MS);
	const now = Date.now();
	const bad = invalid.get(ip);
	if (bad && bad.until > now && bad.count >= INVALID_LOOKUPS_PER_MINUTE)
		throw new ApiError(429, 'Too many invalid kill feed tokens.', 'rate_limited');
	if (!token) return noteInvalid(ip, now);
	const serverId = await lookup(token);
	if (serverId) return serverId;
	return noteInvalid(ip, now);
}

function noteInvalid(ip: string, now: number): null {
	const current = invalid.get(ip);
	invalid.set(ip, {
		count: current && current.until > now ? current.count + 1 : 1,
		until: current && current.until > now ? current.until : now + WINDOW_MS
	});
	if (invalid.size > 10_000)
		for (const [key, value] of invalid) if (value.until <= now) invalid.delete(key);
	return null;
}

/** Test-only. */
export function resetFeedAuthLimits(): void {
	invalid.clear();
}
