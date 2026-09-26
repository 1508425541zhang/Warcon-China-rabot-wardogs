import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { route, readJson, apiJson, ApiError } from '$lib/server/http';
import { weaponRestrictionRules } from '$lib/server/db/schema';
import { writeAudit } from '$lib/server/audit';
import { z } from 'zod';
const config = z.object({
	enabled: z.boolean(),
	causes: z
		.array(
			z
				.string()
				.trim()
				.min(1)
				.max(200)
				.regex(/^[A-Za-z0-9_.-]+$/)
		)
		.max(1000),
	groups: z.array(z.enum(['items', 'vehicles', 'buildables'])).max(3)
});
export const POST = route(async ({ locals, params, request }) => {
	const env = getEnv();
	const { server, access, user } = await requireServerCap(
		env,
		locals,
		params.id!,
		'automation.manage'
	);
	if (!access.caps.has('players.moderate') || !access.caps.has('chat.send'))
		throw new ApiError(403, '需要踢出和私信权限。');
	const parsed = config.safeParse(await readJson(request));
	if (!parsed.success) throw new ApiError(400, '来源列表格式不正确，请每项填写完整来源 ID。');
	if (parsed.data.enabled && !parsed.data.causes.length && !parsed.data.groups.length)
		throw new ApiError(400, '请至少选择一个受限来源。');
	const values = {
		serverId: server.id,
		...parsed.data,
		causes: [...new Set(parsed.data.causes)],
		groups: [...new Set(parsed.data.groups)],
		updatedAt: new Date()
	};
	await env.db
		.insert(weaponRestrictionRules)
		.values(values)
		.onConflictDoUpdate({ target: weaponRestrictionRules.serverId, set: values });
	await writeAudit(env, request, {
		actor: user,
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'weapon_restriction.settings',
		outcome: 'ok',
		detail: values
	});
	return apiJson({ ok: true });
});
