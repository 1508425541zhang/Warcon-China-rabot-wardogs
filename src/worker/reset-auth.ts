// bun run auth:reset -- <username>
// The escape hatch for the one person nobody else can reset: the panel's last owner (or anyone
// else, run by whoever has the box). Removes the authenticator app, every passkey and the recovery
// key, sets a fresh temporary password that must be changed at the next sign-in, signs every
// session out, and prints the password once. Discord and Steam links stay.
import { randomBytes } from 'node:crypto';
import { eq } from 'drizzle-orm';
import { hashPassword } from 'better-auth/crypto';
import { connect } from '$lib/server/db';
import { account, passkey, session, twoFactor, user } from '$lib/server/db/schema';

const username = (process.argv[2] ?? '').trim().toLowerCase();
if (!username) {
	console.error('Usage: bun run auth:reset -- <username>');
	process.exit(2);
}
const url = process.env.DATABASE_URL;
const target =
	url ||
	(process.env.PGHOST && process.env.PGPASSWORD !== undefined
		? {
				hostname: process.env.PGHOST,
				port: Number(process.env.PGPORT) || 5432,
				username: process.env.PGUSER || 'warcon',
				password: process.env.PGPASSWORD,
				database: process.env.PGDATABASE || 'warcon'
			}
		: null);
if (!target) {
	console.error('Set DATABASE_URL, or PGHOST and PGPASSWORD.');
	process.exit(2);
}
const { client, db } = connect(target);
const [u] = await db.select().from(user).where(eq(user.username, username)).limit(1);
if (!u) {
	console.error(`No user "${username}".`);
	await client.end();
	process.exit(1);
}
// 16 random bytes in base64url: 22 characters, comfortably above the 10 minimum.
const password = randomBytes(16).toString('base64url');
const hash = await hashPassword(password);
await db.transaction(async (tx) => {
	await tx.delete(twoFactor).where(eq(twoFactor.userId, u.id));
	await tx.delete(passkey).where(eq(passkey.userId, u.id));
	await tx.delete(session).where(eq(session.userId, u.id));
	const existing = await tx
		.select({ id: account.id, providerId: account.providerId })
		.from(account)
		.where(eq(account.userId, u.id));
	const credential = existing.find((a) => a.providerId === 'credential');
	if (credential) {
		await tx
			.update(account)
			.set({ password: hash, updatedAt: new Date() })
			.where(eq(account.id, credential.id));
	} else {
		await tx.insert(account).values({
			id: crypto.randomUUID(),
			accountId: u.id,
			providerId: 'credential',
			issuer: 'local:credential',
			userId: u.id,
			password: hash
		});
	}
	await tx
		.update(user)
		.set({
			twoFactorEnabled: false,
			recoveryKeyHash: null,
			recoveryKeyAt: null,
			authComplete: false,
			mustChangePassword: true,
			banned: false,
			banReason: null,
			banExpires: null,
			updatedAt: new Date()
		})
		.where(eq(user.id, u.id));
});
console.log(`[warcon] sign-in methods reset for @${username}`);
console.log(`[warcon] temporary password (change it at the next sign-in): ${password}`);
console.log(
	'[warcon] authenticator app, passkeys, recovery key and sessions removed; Discord/Steam links kept'
);
await client.end();
