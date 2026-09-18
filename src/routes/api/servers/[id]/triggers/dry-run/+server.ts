// {kind, config}: replays the last 24 hours against a rule (saved or not) without acting.
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { gateway } from '$lib/server/gateway';
import { dryRun, isTriggerKind, requireRuleCaps } from '$lib/server/triggers';

export const POST = route(async (event) => {
	const env = getEnv();
	const { server, access } = await requireServerCap(
		env,
		event.locals,
		param(event, 'id'),
		'automation.manage'
	);
	const body = await readJson(event.request);
	if (!isTriggerKind(body.kind)) throw new ApiError(400, 'Unknown trigger kind.');
	// A dry run names the players a rule would act on and why: for those who could save the rule.
	requireRuleCaps(body.kind, body.config, server, access);
	const reserved = () =>
		gateway()
			.run(env, server, 'reserved', {})
			.then((r) => (r as { reserved: string[] }).reserved);
	return apiJson({ ok: true, result: await dryRun(env, server, body.kind, body.config, reserved) });
});
