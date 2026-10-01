import { error } from '@sveltejs/kit';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { normalizeError, ApiError } from '$lib/server/http';
import { bindingsView } from '$lib/server/qq/bindings-admin';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({locals,params,url}) => {
	try {
		const env = getEnv();
		const {server,user,access} = await requireServerCap(env,locals,params.id,'players.moderate');
		if (user.apiKey) throw new ApiError(403,'请使用管理员网页账号。');
		return {bindings:await bindingsView(env,server.id,url.searchParams.get('q')??'',Number(url.searchParams.get('page'))),canManage:access.caps.has('automation.manage')};
	} catch (err) {
		const known = normalizeError(err); if (!known) throw err;
		error(known.status,known.message);
	}
};
