import { and, eq, sql } from 'drizzle-orm';
import type { Env } from '../env';
import type { SessionUser } from '../access';
import { writeAudit } from '../audit';
import { integrityModelState, integrityWeaponMap } from '../db/schema';
import { gateway } from '../gateway';
import { ApiError, str } from '../http';
import { DEFAULT_WEAPON_MAP, isWeaponCategory, type WeaponCategory } from './weapons';

export async function weaponMappings(env: Env, orgId: string) {
	return env.db.select().from(integrityWeaponMap).where(eq(integrityWeaponMap.orgId, orgId));
}

const overrideCache = new Map<string, { until: number; values: Map<string, WeaponCategory> }>();
export function invalidateWeaponMap(orgId: string): void {
	overrideCache.delete(orgId);
}

export async function weaponOverrides(
	env: Env,
	orgId: string
): Promise<Map<string, WeaponCategory>> {
	const hit = overrideCache.get(orgId);
	if (hit && hit.until > Date.now()) return hit.values;
	const values = new Map(
		(await weaponMappings(env, orgId)).map((row) => [row.cause, row.category as WeaponCategory])
	);
	overrideCache.set(orgId, { until: Date.now() + 30_000, values });
	return values;
}

function requireCause(input: unknown): string {
	const cause = str(input, 200);
	if (!cause || cause.length > 180 || !/^[A-Za-z0-9_.-]+$/.test(cause))
		throw new ApiError(400, 'Cause must be an exact server-feed tag (1–180 characters).');
	return cause;
}

export async function putWeaponMapping(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	body: Record<string, unknown>
) {
	const cause = requireCause(body.cause);
	const category = body.category;
	if (!isWeaponCategory(category)) throw new ApiError(400, 'Unknown weapon category.');
	const row = await env.db.transaction(async (tx) => {
		const [saved] = await tx
			.insert(integrityWeaponMap)
			.values({ orgId, cause, category, updatedBy: actor.id })
			.onConflictDoUpdate({
				target: [integrityWeaponMap.orgId, integrityWeaponMap.cause],
				set: { category, updatedBy: actor.id, updatedAt: new Date() }
			})
			.returning();
		await tx
			.insert(integrityModelState)
			.values({ orgId, weaponMapVersion: 2, baselineStatus: 'STALE' })
			.onConflictDoUpdate({
				target: integrityModelState.orgId,
				set: {
					weaponMapVersion: sql`${integrityModelState.weaponMapVersion} + 1`,
					baselineStatus: 'STALE',
					updatedAt: new Date()
				}
			});
		return saved;
	});
	overrideCache.delete(orgId);
	await gateway().integrityChanged(orgId);
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.weapon_map.set',
		target: cause,
		outcome: 'ok',
		message: `Classified ${cause} as ${category}`,
		detail: { cause, category }
	});
	return row;
}

export async function deleteWeaponMapping(
	env: Env,
	req: Request,
	actor: SessionUser,
	orgId: string,
	input: unknown
) {
	const cause = requireCause(input);
	const deleted = await env.db.transaction(async (tx) => {
		const [removed] = await tx
			.delete(integrityWeaponMap)
			.where(and(eq(integrityWeaponMap.orgId, orgId), eq(integrityWeaponMap.cause, cause)))
			.returning();
		if (!removed) return null;
		await tx
			.insert(integrityModelState)
			.values({ orgId, weaponMapVersion: 2, baselineStatus: 'STALE' })
			.onConflictDoUpdate({
				target: integrityModelState.orgId,
				set: {
					weaponMapVersion: sql`${integrityModelState.weaponMapVersion} + 1`,
					baselineStatus: 'STALE',
					updatedAt: new Date()
				}
			});
		return removed;
	});
	if (!deleted) throw new ApiError(404, 'Weapon mapping not found.');
	overrideCache.delete(orgId);
	await gateway().integrityChanged(orgId);
	await writeAudit(env, req, {
		actor,
		orgId,
		category: 'org',
		action: 'integrity.weapon_map.remove',
		target: cause,
		outcome: 'ok',
		message: `Removed override for ${cause}`,
		detail: { cause, fallback: DEFAULT_WEAPON_MAP[cause] ?? 'UNKNOWN' }
	});
}
