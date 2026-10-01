import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { route, readJson, apiJson } from '$lib/server/http';
import { changeScheduledTasks, scheduledTasksView } from '$lib/server/scheduled-tasks';
import { writeAudit } from '$lib/server/audit';
import { gateway } from '$lib/server/gateway';
export const GET = route(async ({ locals, params }) => {
	const env = getEnv();
	const { server } = await requireServerCap(env, locals, params.id!, 'automation.manage');
	return apiJson(await scheduledTasksView(env, server.id));
});
export const POST = route(async ({ locals, params, request }) => {
	const env = getEnv();
	const { server, access, user } = await requireServerCap(
		env,
		locals,
		params.id!,
		'automation.manage'
	);
	const body = await readJson(request);
	const value = await changeScheduledTasks(env, server, access, user, body);
	await writeAudit(env, request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'schedule.' + String(body.operation),
		outcome: 'ok',
		detail: body
	});
	gateway().observeSoon(server.id);
	return apiJson(value);
});
