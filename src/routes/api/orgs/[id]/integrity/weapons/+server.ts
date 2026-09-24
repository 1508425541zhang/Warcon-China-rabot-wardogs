import { getEnv } from '$lib/server/env';
import { apiJson, param, readJson, route } from '$lib/server/http';
import { requireOrgRole } from '$lib/server/access';
import { DEFAULT_WEAPON_MAP, WEAPON_CATEGORIES } from '$lib/server/integrity/weapons';
import {
	deleteWeaponMapping,
	putWeaponMapping,
	weaponMappings
} from '$lib/server/integrity/weapon-map';

export const GET = route(async (event) => {
	const env = getEnv();
	const { org } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	return apiJson({
		ok: true,
		categories: WEAPON_CATEGORIES,
		defaults: DEFAULT_WEAPON_MAP,
		overrides: await weaponMappings(env, org.id)
	});
});

export const PUT = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const mapping = await putWeaponMapping(
		env,
		event.request,
		user,
		org.id,
		await readJson(event.request)
	);
	return apiJson({ ok: true, mapping });
});

export const DELETE = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const body = await readJson(event.request);
	await deleteWeaponMapping(env, event.request, user, org.id, body.cause);
	return apiJson({ ok: true });
});
