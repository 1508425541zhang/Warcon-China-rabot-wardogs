// What sign-in methods an account holds, and whether that satisfies the rules in $lib/enrolment.
// The verdict is cached on user.auth_complete so the request hook needs no extra query; call
// refreshAuthComplete after anything that adds or removes a method.
import { count, eq } from 'drizzle-orm';
import type { Env } from './env';
import { account, passkey, user } from './db/schema';
import { settings } from './settings';
import {
	assessEnrolment,
	enrolmentStatus,
	type AuthMethods,
	type Enrolment,
	type EnrolmentStatus,
	type EnrolmentSubject
} from '$lib/enrolment';

export async function authMethodsFor(env: Env, userId: string): Promise<AuthMethods> {
	const [providers, [pk], [u]] = await Promise.all([
		env.db
			.select({ providerId: account.providerId })
			.from(account)
			.where(eq(account.userId, userId)),
		env.db.select({ n: count() }).from(passkey).where(eq(passkey.userId, userId)),
		env.db
			.select({ twoFactorEnabled: user.twoFactorEnabled, recoveryKeyHash: user.recoveryKeyHash })
			.from(user)
			.where(eq(user.id, userId))
			.limit(1)
	]);
	const list = providers.map((p) => p.providerId);
	return {
		password: list.includes('credential'),
		twoFactor: !!u?.twoFactorEnabled,
		passkeys: pk?.n ?? 0,
		providers: list,
		recoveryKey: !!u?.recoveryKeyHash
	};
}

export interface EnrolmentView {
	methods: AuthMethods;
	enrolment: Enrolment;
}

/** Re-evaluates the rules for one account and stores the verdict. */
export async function refreshAuthComplete(env: Env, userId: string): Promise<EnrolmentView> {
	const [row] = await env.db
		.select({ role: user.role, authComplete: user.authComplete })
		.from(user)
		.where(eq(user.id, userId))
		.limit(1);
	const methods = await authMethodsFor(env, userId);
	const enrolment = assessEnrolment(methods, row?.role === 'owner' ? 'owner' : 'member');
	if (row && row.authComplete !== enrolment.complete)
		await env.db.update(user).set({ authComplete: enrolment.complete }).where(eq(user.id, userId));
	return { methods, enrolment };
}

/** The grace period counts from the first sign-in after the rules arrived. */
export async function startGrace(env: Env, userId: string): Promise<void> {
	await env.db
		.update(user)
		.set({ authGraceStartedAt: new Date() })
		.where(eq(user.id, userId))
		.then(() => {})
		.catch(() => {});
}

export const statusFor = (u: EnrolmentSubject): EnrolmentStatus => enrolmentStatus(u, settings());
