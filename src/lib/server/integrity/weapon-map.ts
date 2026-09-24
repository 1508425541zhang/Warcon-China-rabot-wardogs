import { and, eq } from 'drizzle-orm';
import type { Env } from '../env';
import type { SessionUser } from '../access';
import { writeAudit } from '../audit';
import { integrityWeaponMap } from '../db/schema';
import { ApiError, str } from '../http';
import { DEFAULT_WEAPON_MAP, isWeaponCategory, type WeaponCategory } from './weapons';

export async function weaponMappings(env: Env, orgId: string) {
	return env.db.select().from(integrityWeaponMap).where(eq(integrityWeaponMap.orgId, orgId));
}

export async function weaponOverrides(
	env: Env,
	orgId: string
): Promise<Map<string, WeaponCategory>> {
	return new Map(
		(await weaponMappings(env, orgId)).map((row) => [row.cause, row.category as WeaponCategory])
	);
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
	const [row] = await env.db
		.insert(integrityWeaponMap)
		.values({ orgId, cause, category, updatedBy: actor.id })
		.onConflictDoUpdate({
			target: [integrityWeaponMap.orgId, integrityWeaponMap.cause],
			set: { category, updatedBy: actor.id, updatedAt: new Date() }
		})
		.returning();
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
	const [deleted] = await env.db
		.delete(integrityWeaponMap)
		.where(and(eq(integrityWeaponMap.orgId, orgId), eq(integrityWeaponMap.cause, cause)))
		.returning();
	if (!deleted) throw new ApiError(404, 'Weapon mapping not found.');
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
