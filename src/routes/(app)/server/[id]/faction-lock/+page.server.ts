import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { factionLockView } from '$lib/server/faction-lock';
import { serverLive } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import type { Status } from '$lib/types';
export const load = async ({ locals, params }: import('./$types').PageServerLoadEvent) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, locals, params.id, 'automation.manage');
	const [live] = await env.db.select().from(serverLive).where(eq(serverLive.serverId, server.id));
	return {
		...(await factionLockView(env, server.id)),
		teams: (live?.status as Status | null)?.scores?.map((s) => s.name) ?? []
	};
};
