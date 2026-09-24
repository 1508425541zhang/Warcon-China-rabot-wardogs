import { eq, sql } from 'drizzle-orm';
import type { Env } from '../env';
import type { SessionUser } from '../access';
import { writeAudit } from '../audit';
import { integrityRules } from '../db/schema';
import { ApiError } from '../http';
import { DEFAULT_INTEGRITY_RULES, type IntegrityRuleConfig } from './score';

export interface RuleSet {
	version: number;
	config: IntegrityRuleConfig;
}

const bounds: Record<string, [number, number]> = {
	repeatWindowMinutes: [1, 120],
	repeatSecond: [0, 30],
	repeatThird: [0, 30],
	repeatBoth5: [0, 30],
	repeatBoth6: [0, 30],
	repeatKo: [0, 30],
	steamPriorCap: [0, 30],
	recentVac: [0, 30],
	recentGameBan: [0, 30],
	oldVac: [0, 30],
	oldGameBan: [0, 30],
	lowPlaytimeHours: [0, 100],
	lowPlaytimeKpm: [1, 20],
	lowPlaytimePoints: [0, 15],
	headshotMinKills: [1, 100],
	headshotMinPct: [1, 100],
	headshotMax: [0, 10],
	penetrationMinKills: [1, 100],
	penetrationMinPct: [1, 100],
	penetrationMax: [0, 6],
	burstMax: [1, 20],
	passiveWatchThreshold: [1, 99],
	activeWatchThreshold: [1, 99],
	koThreshold: [1, 100],
	quarantineThreshold: [1, 100],
	quarantineDays: [1, 3650]
};

/** Validate every effective value, including values inherited from the previous version. */
export function validateIntegrityRules(
	patch: Record<string, unknown>,
	base: IntegrityRuleConfig = DEFAULT_INTEGRITY_RULES
): IntegrityRuleConfig {
	const allowed = new Set(Object.keys(DEFAULT_INTEGRITY_RULES));
	for (const key of Object.keys(patch))
		if (!allowed.has(key)) throw new ApiError(400, `Unknown integrity rule '${key}'.`);
	const next = { ...base, ...patch } as IntegrityRuleConfig;
	for (const [key, [min, max]] of Object.entries(bounds)) {
		const value = next[key as keyof IntegrityRuleConfig];
		if (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max)
			throw new ApiError(400, `${key} must be between ${min} and ${max}.`);
	}
	for (const key of ['kpmBands', 'uniqueVictimBands', 'reportBands'] as const) {
		const bands = next[key];
		if (
			!Array.isArray(bands) ||
			!bands.length ||
			bands.length > 10 ||
			bands.some(
				(band, i) =>
					!band ||
					typeof band.min !== 'number' ||
					!Number.isFinite(band.min) ||
					band.min < 1 ||
					typeof band.points !== 'number' ||
					!Number.isInteger(band.points) ||
					band.points < 0 ||
					band.points > 100 ||
					(i > 0 && (band.min <= bands[i - 1].min || band.points < bands[i - 1].points))
			)
		)
			throw new ApiError(400, `${key} must contain ascending thresholds and weights.`);
	}
	if (next.mode !== 'dry_run')
		throw new ApiError(400, 'Integrity enforcement is not available in this phase.');
	if (!(
		next.passiveWatchThreshold < next.activeWatchThreshold &&
		next.activeWatchThreshold < next.koThreshold &&
		next.koThreshold < next.quarantineThreshold
	))
		throw new ApiError(400, 'Risk thresholds must increase from watch to quarantine.');
	if (next.oldVac > next.recentVac || next.oldGameBan > next.recentGameBan)
		throw new ApiError(400, 'Old Steam ban weights cannot exceed recent weights.');
	return next;
}

const cache = new Map<string, { until: number; rules: RuleSet }>();

/** Insert the default version once for both existing and newly created organisations. */
export async function getIntegrityRules(env: Env, orgId: string): Promise<RuleSet> {
	const hit = cache.get(orgId);
	if (hit && hit.until > Date.now()) return hit.rules;
	await env.db
		.insert(integrityRules)
		.values({ orgId, config: DEFAULT_INTEGRITY_RULES })
		.onConflictDoNothing({ target: integrityRules.orgId });
	const [row] = await env.db.select().from(integrityRules).where(eq(integrityRules.orgId, orgId));
	if (!row) throw new Error('Integrity rules were not saved.');
	const rules = {
		version: row.version,
		config: validateIntegrityRules(row.config as Record<string, unknown>)
	};
	cache.set(orgId, { until: Date.now() + 30_000, rules });
	return rules;
}

export async function saveIntegrityRules(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	patch: Record<string, unknown>
): Promise<RuleSet> {
	if (!Object.keys(patch).length) throw new ApiError(400, 'No integrity rule changes supplied.');
	cache.delete(orgId);
	const before = await getIntegrityRules(env, orgId);
	const config = validateIntegrityRules(patch, before.config);
	const [row] = await env.db
		.update(integrityRules)
		.set({
			config,
			version: sql`${integrityRules.version} + 1`,
			updatedBy: actor.id,
			updatedAt: new Date()
		})
		.where(eq(integrityRules.orgId, orgId))
		.returning();
	if (!row) throw new Error('Integrity rules were not saved.');
	cache.delete(orgId);
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.rules.update',
		outcome: 'ok',
		message: `Integrity rules version ${row.version}`,
		detail: { beforeVersion: before.version, version: row.version, patch }
	});
	return { version: row.version, config };
}
