import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { apiJson, param, readJson, route, ApiError } from '$lib/server/http';
import { bindingsView, manageBinding } from '$lib/server/qq/bindings-admin';

export const GET = route(async event => {
	const env = getEnv();
	const {server,user} = await requireServerCap(env,event.locals,param(event,'id'),'players.moderate');
	if (user.apiKey) throw new ApiError(403,'请使用管理员网页账号。');
	return apiJson(await bindingsView(env,server.id,event.url.searchParams.get('q')??'',Number(event.url.searchParams.get('page'))));
});
export const POST = route(async event => {
	const env = getEnv();
	const {server,user,access} = await requireServerCap(env,event.locals,param(event,'id'),'automation.manage');
	if (user.apiKey || !access.caps.has('players.moderate')) throw new ApiError(403,'需要自动化管理和玩家管理权限。');
	return apiJson(await manageBinding(env,server,user,await readJson(event.request)));
});
