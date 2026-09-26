import { groupConfigSchema } from '$lib/group-control-policy';
import { groupControlRules } from '$lib/server/db/schema';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { apiJson, param, readJson, route, ApiError } from '$lib/server/http';
import { writeAudit } from '$lib/server/audit';
import { scanGroups } from '$lib/server/group-control';
export const POST = route(async (event) => {
	const env = getEnv();
	const { server, user } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	const body = await readJson(event.request);
	if (body?.operation === 'scan') {
		await scanGroups(env, server, true);
	} else {
		const parsed = groupConfigSchema.safeParse(body?.config);
		if (!parsed.success)
			throw new ApiError(400, '请输入有效配置：前缀2～16字符，相似度0～99%，人数2～100。');
		const row = { serverId: server.id, config: parsed.data, updatedAt: new Date() };
		await env.db
			.insert(groupControlRules)
			.values(row)
			.onConflictDoUpdate({ target: groupControlRules.serverId, set: row });
	}
	await writeAudit(env, event.request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: body?.operation === 'scan' ? 'group_control.scan' : 'group_control.settings',
		outcome: 'ok',
		detail: body?.operation === 'scan' ? {} : groupConfigSchema.parse(body?.config)
	});
	return apiJson({ ok: true });
});
