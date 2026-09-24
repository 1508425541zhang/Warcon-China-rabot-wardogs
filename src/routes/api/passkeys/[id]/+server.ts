import { getEnv } from '$lib/server/env';
import { apiJson, param, route } from '$lib/server/http';
import { requireUser } from '$lib/server/access';
import { writeAudit } from '$lib/server/audit';
import { deletePasskey } from '$lib/server/users';

export const DELETE = route(async (event) => {
	const env = getEnv();
	const user = requireUser(event.locals);
	const removed = await deletePasskey(env, user.id, param(event, 'id'));
	await writeAudit(env, event.request, {
		actor: user,
		category: 'auth',
		action: 'passkey.remove',
		outcome: 'ok',
		target: removed.name
	});
	return apiJson({ ok: true });
});
