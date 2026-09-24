// Recovery keys: a long one-time secret shown once, for the person nobody else can reset (the sole
// owner of an organisation, or of the whole panel). The panel stores only its hash; using the key
// signs the account in, consumes it, and sends the user to the account page to set things up again.
import { createHash, randomInt } from 'node:crypto';
import { and, eq } from 'drizzle-orm';
import type { Env } from './env';
import { timingSafeEqualStr } from './crypto';
import { user } from './db/schema';

// No 0/O, 1/I/L: the key is meant to be printed and typed back.
const ALPHABET = 'ABCDEFGHJKMNPQRSTVWXYZ23456789';
const GROUPS = 8;
const GROUP_LEN = 5;

/** e.g. "K7Q2M-9XWRT-…" (8 groups of 5 = 40 symbols ≈ 196 bits). */
export function generateRecoveryKey(): string {
	const groups: string[] = [];
	for (let g = 0; g < GROUPS; g++) {
		let s = '';
		for (let i = 0; i < GROUP_LEN; i++) s += ALPHABET[randomInt(ALPHABET.length)];
		groups.push(s);
	}
	return groups.join('-');
}

/** Upper-cases, drops separators and forgives the look-alikes the alphabet avoids. */
export function normalizeRecoveryKey(input: string): string {
	return input
		.toUpperCase()
		.replace(/[^A-Z0-9]/g, '')
		.replace(/O/g, '0')
		.replace(/[IL]/g, '1');
}

export const hashRecoveryKey = (key: string): string =>
	createHash('sha256').update(normalizeRecoveryKey(key)).digest('hex');

/** Issues a fresh key (replacing any earlier one) and returns it: the only time it is readable. */
export async function issueRecoveryKey(env: Env, userId: string): Promise<string> {
	const key = generateRecoveryKey();
	await env.db
		.update(user)
		.set({
			recoveryKeyHash: hashRecoveryKey(key),
			recoveryKeyAt: new Date(),
			updatedAt: new Date()
		})
		.where(eq(user.id, userId));
	return key;
}

export async function clearRecoveryKey(env: Env, userId: string): Promise<void> {
	await env.db
		.update(user)
		.set({ recoveryKeyHash: null, recoveryKeyAt: null, updatedAt: new Date() })
		.where(eq(user.id, userId));
}

/**
 * Checks a key against the named account and, when it matches, consumes it. Returns the user row
 * on success and null on any miss; the caller counts misses against the login lockout.
 */
export async function consumeRecoveryKey(env: Env, username: string, key: string) {
	const [row] = await env.db
		.select()
		.from(user)
		.where(eq(user.username, username.toLowerCase()))
		.limit(1);
	const stored = row?.recoveryKeyHash ?? '';
	const given = hashRecoveryKey(key);
	// Compare even without a row so a missing account costs the same as a wrong key.
	const ok = timingSafeEqualStr(stored || '0'.repeat(64), given) && !!stored && !!row;
	if (!ok || !row) return null;
	const consumed = await env.db
		.update(user)
		.set({ recoveryKeyHash: null, recoveryKeyAt: null, updatedAt: new Date() })
		.where(and(eq(user.id, row.id), eq(user.recoveryKeyHash, stored)))
		.returning({ id: user.id });
	return consumed.length ? row : null;
}
