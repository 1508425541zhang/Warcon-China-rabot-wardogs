// "Which of the org's servers" for the things that can be held to some of them: API keys and
// Discord webhooks. null means every server and has to be said; a list has to name at least one
// server of the org. An empty or malformed list is refused rather than read as "all", which is
// the opposite of what whoever sent it meant.
import { and, eq, inArray } from 'drizzle-orm';
import type { Env } from './env';
import { ApiError, str } from './http';
import { servers } from './db/schema';

export async function parseServerScope(
	env: Env,
	orgId: string,
	raw: unknown
): Promise<string[] | null> {
	if (raw === null || raw === undefined) return null;
	const wanted =
		Array.isArray(raw) && raw.every((v) => typeof v === 'string')
			? [...new Set(raw.map((v) => str(v, 64)).filter(Boolean))]
			: [];
	if (!wanted.length)
		throw new ApiError(400, 'Pick at least one server, or choose every server.', 'bad_scope');
	const known = await env.db
		.select({ id: servers.id })
		.from(servers)
		.where(and(eq(servers.orgId, orgId), inArray(servers.id, wanted)));
	if (known.length !== wanted.length)
		throw new ApiError(400, 'One of those servers is not in this organisation.', 'bad_scope');
	return known.map((s) => s.id);
}
