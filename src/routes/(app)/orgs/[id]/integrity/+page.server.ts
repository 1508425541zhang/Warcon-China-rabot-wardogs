import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { getIntegrityRules } from '$lib/server/integrity/rules';
import { weaponMappings } from '$lib/server/integrity/weapon-map';
import { DEFAULT_INTEGRITY_RULES } from '$lib/server/integrity/score';
import { DEFAULT_WEAPON_MAP, WEAPON_CATEGORIES } from '$lib/server/integrity/weapons';
import { integrityBaselines } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { shadowComparison } from '$lib/server/integrity/baselines';
import { integrityImports } from '$lib/server/integrity/imports';

export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { org } = await requireOrgRole(env, locals, params.id, 'owner');
		const [rules, mappings, baselines, imports] = await Promise.all([
			getIntegrityRules(env, org.id),
			weaponMappings(env, org.id),
			env.db.select().from(integrityBaselines).where(eq(integrityBaselines.orgId, org.id)),
			integrityImports(env, org.id)
		]);
		const comparison = await shadowComparison(env, org.id, rules.config.koThreshold);
		return {
			orgId: org.id,
			ruleVersion: rules.version,
			assessmentMode: rules.assessmentMode,
			comparison,
			imports: imports.map((row) => ({
				...row,
				firstEventAt: row.firstEventAt.toISOString(),
				lastEventAt: row.lastEventAt.toISOString(),
				stagedAt: row.stagedAt.toISOString(),
				reviewedAt: row.reviewedAt?.toISOString() ?? null
			})),
			baselineSummary: {
				metrics: new Set(
					baselines
						.filter((row) => row.sampleCount >= (row.source === 'external' ? 30 : 200))
						.map((row) => row.metric)
				).size,
				externalMetrics: new Set(
					baselines
						.filter((row) => row.source === 'external' && row.sampleCount >= 30)
						.map((row) => row.metric)
				).size,
				maxSamples: Math.max(0, ...baselines.map((row) => row.sampleCount)),
				calculatedAt: baselines[0]?.calculatedAt.toISOString() ?? null
			},
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
