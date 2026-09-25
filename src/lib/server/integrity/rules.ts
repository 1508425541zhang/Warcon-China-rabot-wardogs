import { eq, sql } from 'drizzle-orm';
import type { Env } from '../env';
import type { Tx } from '../db';
import type { SessionUser } from '../access';
import { writeAudit } from '../audit';
import { integrityRules } from '../db/schema';
import { ApiError } from '../http';
import { DEFAULT_INTEGRITY_RULES, type IntegrityRuleConfig } from './score';
import { DEFAULT_ENFORCEMENT, type EnforcementSettings } from './decisions';
import type { AssessmentMode } from './statistics';

export interface RuleSet {
	version: number;
	config: IntegrityRuleConfig;
	enforcement: EnforcementSettings;
	assessmentMode: AssessmentMode;
}

const bounds: Record<string, [number, number]> = {
	repeatWindowMinutes: [1, 120],
	repeatSecond: [0, 30],
	repeatThird: [0, 30],
	repeatBoth5: [0, 30],
	repeatBoth6: [0, 30],
	repeatKo: [0, 30],
	repeatKoWindowHours: [1, 720],
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
	burstFindingMin: [1, 12],
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
	// Existing JSON rows predate newly added fields; defaults remain effective without a migration.
	const next = { ...DEFAULT_INTEGRITY_RULES, ...base, ...patch } as IntegrityRuleConfig;
	for (const [key, [min, max]] of Object.entries(bounds)) {
		const value = next[key as keyof IntegrityRuleConfig];
		if (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max)
			throw new ApiError(400, `${key} must be between ${min} and ${max}.`);
	}
	if (!Number.isInteger(next.burstFindingMin) || !Number.isInteger(next.repeatKoWindowHours))
		throw new ApiError(400, 'Burst and repeat KO periods must be whole numbers.');
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
const enforcementOf = (row: typeof integrityRules.$inferSelect | undefined): EnforcementSettings =>
	row
		? {
				autoKickEnabled: row.autoKickEnabled,
				autoQuarantine24hEnabled: row.autoQuarantine24hEnabled,
				autoQuarantine7dEnabled: row.autoQuarantine7dEnabled,
				autoActionMaxPerHour: row.autoActionMaxPerHour,
				autoActionMaxPercentOnline: row.autoActionMaxPercentOnline,
				autoSuspendedAt: row.autoSuspendedAt
			}
		: DEFAULT_ENFORCEMENT;

/** Serializes saves even before an org has its first integrity_rules row. */
async function lockRules(tx: Tx, orgId: string) {
	await tx.execute(
		sql`SELECT pg_advisory_xact_lock(hashtextextended(${`integrity-rules:${orgId}`}, 0))`
	);
	const [row] = await tx
		.select()
		.from(integrityRules)
		.where(eq(integrityRules.orgId, orgId))
		.for('update');
	return row;
}

/** Reads never insert defaults; an org with no row uses an in-memory version 1. */
export async function getIntegrityRules(env: Env, orgId: string): Promise<RuleSet> {
	const hit = cache.get(orgId);
	if (hit && hit.until > Date.now()) return hit.rules;
	const [row] = await env.db.select().from(integrityRules).where(eq(integrityRules.orgId, orgId));
	const rules = {
		version: row?.version ?? 1,
		assessmentMode: (row?.assessmentMode ?? 'statistical_shadow') as AssessmentMode,
		config: row
			? validateIntegrityRules(row.config as Record<string, unknown>)
			: DEFAULT_INTEGRITY_RULES,
		enforcement: enforcementOf(row)
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
	if ('mode' in patch || 'quarantineDays' in patch)
		throw new ApiError(
			400,
			'Legacy mode and quarantineDays cannot be changed. Use enforcement switches for actions and duration.'
		);
	const saved = await env.db.transaction(async (tx) => {
		const before = await lockRules(tx, orgId);
		const config = validateIntegrityRules(
			patch,
			before
				? validateIntegrityRules(before.config as Record<string, unknown>)
				: DEFAULT_INTEGRITY_RULES
		);
		const [row] = before
			? await tx
					.update(integrityRules)
					.set({
						config,
						version: sql`${integrityRules.version} + 1`,
						updatedBy: actor.id,
						updatedAt: new Date()
					})
					.where(eq(integrityRules.orgId, orgId))
					.returning()
			: await tx.insert(integrityRules).values({ orgId, config, updatedBy: actor.id }).returning();
		if (!row) throw new Error('Integrity rules were not saved.');
		return { row, beforeVersion: before?.version ?? 1 };
	});
	cache.delete(orgId);
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.rules.update',
		outcome: 'ok',
		message: `Integrity rules version ${saved.row.version}`,
		detail: { beforeVersion: saved.beforeVersion, version: saved.row.version, patch }
	});
	return {
		version: saved.row.version,
		assessmentMode: saved.row.assessmentMode as AssessmentMode,
		config: validateIntegrityRules(saved.row.config as Record<string, unknown>),
		enforcement: enforcementOf(saved.row)
	};
}

/** Owner-only route calls this separately; changing engines invalidates old case rule versions. */
export async function saveAssessmentMode(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	mode: AssessmentMode,
	confirmation: string
): Promise<RuleSet> {
	if (!['legacy', 'statistical_shadow', 'statistical'].includes(mode))
		throw new ApiError(400, 'Unknown Integrity assessment mode.');
	if (mode === 'statistical' && confirmation !== 'ENABLE_STATISTICAL_INTEGRITY')
		throw new ApiError(400, 'Explicit statistical enforcement confirmation is required.');
	const saved = await env.db.transaction(async (tx) => {
		const current = await lockRules(tx, orgId);
		const [row] = current
			? await tx
					.update(integrityRules)
					.set({
						assessmentMode: mode,
						version: sql`${integrityRules.version} + 1`,
						updatedBy: actor.id,
						updatedAt: new Date()
					})
					.where(eq(integrityRules.orgId, orgId))
					.returning()
			: await tx
					.insert(integrityRules)
					.values({
						orgId,
						config: DEFAULT_INTEGRITY_RULES,
						assessmentMode: mode,
						updatedBy: actor.id
					})
					.returning();
		return row;
	});
	cache.delete(orgId);
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.assessment_mode.update',
		outcome: 'ok',
		message: `Integrity assessment mode: ${mode}`,
		detail: { mode, version: saved.version }
	});
	return {
		version: saved.version,
		assessmentMode: mode,
		config: validateIntegrityRules(saved.config as Record<string, unknown>),
		enforcement: enforcementOf(saved)
	};
}

/** Enabling any automatic action requires an explicit owner acknowledgement. */
export async function saveIntegrityEnforcement(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	values: Record<string, unknown>
): Promise<EnforcementSettings> {
	const allowed = new Set([
		'autoKickEnabled',
		'autoQuarantine24hEnabled',
		'autoQuarantine7dEnabled',
		'autoActionMaxPerHour',
		'autoActionMaxPercentOnline',
		'resume',
		'confirmation'
	]);
	for (const key of Object.keys(values))
		if (!allowed.has(key)) throw new ApiError(400, `Unknown enforcement setting '${key}'.`);
	if (!Object.keys(values).some((key) => key !== 'confirmation'))
		throw new ApiError(400, 'No enforcement setting changes supplied.');
	const saved = await env.db.transaction(async (tx) => {
		const current = await lockRules(tx, orgId);
		const before = enforcementOf(current);
		const changes: Partial<EnforcementSettings> = {};
		for (const key of [
			'autoKickEnabled',
			'autoQuarantine24hEnabled',
			'autoQuarantine7dEnabled'
		] as const) {
			if (values[key] !== undefined) {
				if (typeof values[key] !== 'boolean') throw new ApiError(400, `${key} must be boolean.`);
				changes[key] = values[key] as boolean;
			}
		}
		for (const key of ['autoActionMaxPerHour', 'autoActionMaxPercentOnline'] as const) {
			if (values[key] !== undefined) {
				const value = values[key];
				if (!Number.isInteger(value) || Number(value) < 1 || Number(value) > 100)
					throw new ApiError(400, `${key} must be between 1 and 100.`);
				changes[key] = Number(value);
			}
		}
		const enabling = (
			['autoKickEnabled', 'autoQuarantine24hEnabled', 'autoQuarantine7dEnabled'] as const
		).some((key) => changes[key] === true && !before[key]);
		if (
			(enabling || values.resume === true) &&
			values.confirmation !== 'ENABLE_EXPERIMENTAL_INTEGRITY'
		)
			throw new ApiError(400, 'Explicit experimental enforcement confirmation is required.');
		if (values.resume === true) changes.autoSuspendedAt = null;
		const [row] = current
			? await tx
					.update(integrityRules)
					.set({ ...changes, updatedBy: actor.id, updatedAt: new Date() })
					.where(eq(integrityRules.orgId, orgId))
					.returning()
			: await tx
					.insert(integrityRules)
					.values({ orgId, config: DEFAULT_INTEGRITY_RULES, updatedBy: actor.id, ...changes })
					.returning();
		if (!row) throw new Error('Integrity enforcement settings were not saved.');
		return { before, row };
	});
	cache.delete(orgId);
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action:
			values.resume === true ? 'integrity.enforcement.resume' : 'integrity.enforcement.update',
		outcome: 'ok',
		message: 'Experimental Integrity enforcement settings updated',
		detail: { before: saved.before, after: enforcementOf(saved.row) }
	});
	return enforcementOf(saved.row);
}
