import { fail } from '@sveltejs/kit';
import { sql } from 'drizzle-orm';
import { requireUser } from '$lib/server/access';
import { getEnv } from '$lib/server/env';
import { publicMessage } from '$lib/server/http';
import { bindAccount } from '$lib/server/qq/identity';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const actor = requireUser(locals);
	const links = await getEnv().db.execute<{ server_id: string; steam_id: string }>(
		sql`SELECT server_id,steam_id FROM qq_links WHERE user_id=${actor.id}`
	);
	return { links };
};
export const actions: Actions = {
	bind: async ({ locals, request }) => {
		const actor = requireUser(locals);
		try {
			await bindAccount(
				getEnv(),
				String((await request.formData()).get('code') || '').trim(),
				actor
			);
			return { message: '绑定成功，可以回 QQ 群使用机器人。' };
		} catch (error) {
			return fail(400, { message: publicMessage(error) });
		}
	},
	unbind: async ({ locals, request }) => {
		const actor = requireUser(locals);
		const serverId = String((await request.formData()).get('serverId') || '');
		await getEnv().db.execute(
			sql`DELETE FROM qq_links WHERE user_id=${actor.id} AND server_id=${serverId}`
		);
		return { message: '已解绑，Steam 账号的积分保留。' };
	}
};
