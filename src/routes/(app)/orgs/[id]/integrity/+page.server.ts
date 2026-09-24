import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { getIntegrityRules } from '$lib/server/integrity/rules';
import { weaponMappings } from '$lib/server/integrity/weapon-map';
import { DEFAULT_INTEGRITY_RULES } from '$lib/server/integrity/score';
import { DEFAULT_WEAPON_MAP, WEAPON_CATEGORIES } from '$lib/server/integrity/weapons';

export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { org } = await requireOrgRole(env, locals, params.id, 'owner');
		const [rules, mappings] = await Promise.all([
			getIntegrityRules(env, org.id),
			weaponMappings(env, org.id)
		]);
		return {
			orgId: org.id,
			ruleVersion: rules.version,
			ruleConfig: rules.config,
			ruleDefaults: DEFAULT_INTEGRITY_RULES,
			enforcement: {
				...rules.enforcement,
				autoSuspendedAt: rules.enforcement.autoSuspendedAt?.toISOString() ?? null
			},
			weaponOverrides: mappings.map((row) => ({ cause: row.cause, category: row.category })),
			weaponDefaults: DEFAULT_WEAPON_MAP,
			weaponCategories: WEAPON_CATEGORIES
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
