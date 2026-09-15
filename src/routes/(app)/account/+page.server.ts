import { fail, redirect } from '@sveltejs/kit';
import QRCode from 'qrcode';
import type { Actions, PageServerLoad } from './$types';
import { discordEnabled, getEnv } from '$lib/server/env';
import { normalizeError, str } from '$lib/server/http';
import { writeAudit } from '$lib/server/audit';
import { requireUser, userOrgs } from '$lib/server/access';
import { clearScopeCookie } from '$lib/server/scope';
import { auditSelfDelete } from '$lib/server/erasure';
import {
	getUser,
	linkedProviders,
	listPasskeys,
	listSessions,
	revokeSession,
	setDefaultOrg,
	setMustChangePassword,
	setSteamId,
	unlinkProvider,
	validatePassword
} from '$lib/server/users';
import { requireSteamId } from '$lib/server/steam';
import { authMethodsFor, refreshAuthComplete, statusFor } from '$lib/server/enrolment';
import { assessEnrolment } from '$lib/enrolment';
import { clearRecoveryKey, issueRecoveryKey } from '$lib/server/recovery';
import { beginSteam } from '$lib/server/steam-auth';

export const load: PageServerLoad = async ({ locals }) => {
	const env = getEnv();
	const user = requireUser(locals);
	const [sessions, row, passkeys, methods] = await Promise.all([
		listSessions(env, user.id, locals.session?.id ?? null),
		getUser(env, user.id),
		listPasskeys(env, user.id),
		authMethodsFor(env, user.id)
	]);
	const enrolment = assessEnrolment(methods, user.role);
	return {
		sessions,
		discord: discordEnabled(env),
		providers: methods.providers,
		/** false for accounts created through Discord, Steam or a passkey: they set a password rather than change one */
		hasPassword: methods.password,
		steamId: row?.steamId ?? '',
		defaultOrgId: row?.defaultOrgId ?? '',
		passkeys,
		methods,
		enrolment,
		status: statusFor({ ...user, authComplete: enrolment.complete }),
		recoveryKeyAt: row?.recoveryKeyAt ? row.recoveryKeyAt.toISOString() : null
	};
};

/** Better Auth's "wrong password" comes back as a message; map it to a clean 403. */
const wrongPassword = (message: string) => /INVALID_PASSWORD|invalid password/i.test(message);

export const actions: Actions = {
	/** Pick the organisation the panel opens scoped to; blank means every org. */
	defaultOrg: async ({ request, locals, cookies }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const raw = str((await request.formData()).get('orgId'), 64);
		const orgs = await userOrgs(env, user);
		const org = raw ? orgs.find((o) => o.id === raw) : null;
		if (raw && !org) return fail(404, { error: 'Organisation not found.' });
		await setDefaultOrg(env, user.id, org?.id ?? null);
		// The new default should take effect at once, so any browser-level override is dropped.
		clearScopeCookie(cookies);
		await writeAudit(env, request, {
			actor: user,
			category: 'user',
			action: 'account.default_org',
			outcome: 'ok',
			target: org?.name ?? '',
			message: org ? `Default organisation: ${org.name}` : 'Default organisation cleared'
		});
		return { defaultOrg: true, orgName: org?.name ?? '' };
	},

	/** Link (or clear) the SteamID64 an organisation may hand a reserved slot to. */
	steam: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const raw = str((await request.formData()).get('steamId'), 32);
		let steamId: string | null = null;
		try {
			steamId = raw ? requireSteamId(raw) : null;
			await setSteamId(env, user.id, steamId);
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			return fail(known.status, { error: known.message });
		}
		await writeAudit(env, request, {
			actor: user,
			category: 'user',
			action: 'account.steam',
			outcome: 'ok',
			target: steamId ?? '',
			message: steamId ? `Linked SteamID ${steamId}` : 'Unlinked SteamID'
		});
		return { steam: true, steamId };
	},

	/** Change the password, or set a first one for an account that has none. */
	password: async ({ request, locals, url }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const form = await request.formData();
		const current = String(form.get('current') || '');
		const next = String(form.get('next') || '');
		if (next !== String(form.get('again') || ''))
			return fail(400, { error: 'New passwords do not match.' });
		const hasPassword = (await linkedProviders(env, user.id)).includes('credential');
		try {
			validatePassword(next);
			if (hasPassword) {
				await locals.auth!.api.changePassword({
					body: { currentPassword: current, newPassword: next, revokeOtherSessions: true },
					headers: request.headers
				});
			} else {
				await locals.auth!.api.setPassword({
					body: { newPassword: next },
					headers: request.headers
				});
			}
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			await writeAudit(env, request, {
				actor: user,
				category: 'auth',
				action: 'password.change',
				outcome: 'denied',
				message: known.message
			});
			return fail(wrongPassword(known.message) ? 403 : known.status, {
				error: wrongPassword(known.message) ? 'Current password is wrong.' : known.message
			});
		}
		await setMustChangePassword(env, user.id, false);
		await refreshAuthComplete(env, user.id);
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: hasPassword ? 'password.change' : 'password.set',
			outcome: 'ok'
		});
		if (url.searchParams.get('force')) redirect(303, '/');
		return hasPassword ? { changed: true } : { set: true };
	},

	/** Drop the password: only for accounts that keep another way in (a passkey or a linked provider). */
	removePassword: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const confirm = str((await request.formData()).get('confirm'), 32);
		if (confirm.toLowerCase() !== user.username.toLowerCase())
			return fail(400, { error: 'Type your username to confirm.' });
		try {
			await unlinkProvider(env, user.id, 'credential');
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			return fail(known.status, { error: known.message });
		}
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: 'password.remove',
			outcome: 'ok'
		});
		return { passwordRemoved: true };
	},

	/** Step one of enrolling an authenticator app: a secret, shown as a QR code and as text. */
	totpStart: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const password = String((await request.formData()).get('password') || '');
		const hasPassword = (await linkedProviders(env, user.id)).includes('credential');
		let totpURI = '';
		try {
			const res = await locals.auth!.api.enableTwoFactor({
				body: { password: hasPassword ? password : undefined, issuer: env.APP_NAME || 'Warcon' },
				headers: request.headers
			});
			totpURI = (res as { totpURI?: string }).totpURI ?? '';
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			return fail(wrongPassword(known.message) ? 403 : known.status, {
				error: wrongPassword(known.message) ? 'Password is wrong.' : known.message
			});
		}
		if (!totpURI) return fail(500, { error: 'No authenticator secret was produced.' });
		const secret = new URL(totpURI).searchParams.get('secret') ?? '';
		const svg = await QRCode.toString(totpURI, { type: 'svg', margin: 1, width: 180 });
		return { totp: { svg, secret, uri: totpURI } };
	},

	/** Step two: the first code proves the app holds the secret; the backup codes are shown once. */
	totpConfirm: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const code = str((await request.formData()).get('code'), 12).replace(/\s+/g, '');
		try {
			await locals.auth!.api.verifyTOTP({ body: { code }, headers: request.headers });
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			await writeAudit(env, request, {
				actor: user,
				category: 'auth',
				action: '2fa.enable',
				outcome: 'denied',
				message: known.message
			});
			return fail(400, {
				error: 'That code was not accepted. Check the app and try again.',
				totpRetry: true
			});
		}
		const codes = await locals.auth!.api.viewBackupCodes({ body: { userId: user.id } });
		await refreshAuthComplete(env, user.id);
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: '2fa.enable',
			outcome: 'ok'
		});
		return { backupCodes: (codes as { backupCodes?: string[] }).backupCodes ?? [], enabled: true };
	},

	totpDisable: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const password = String((await request.formData()).get('password') || '');
		const hasPassword = (await linkedProviders(env, user.id)).includes('credential');
		try {
			await locals.auth!.api.disableTwoFactor({
				body: { password: hasPassword ? password : undefined },
				headers: request.headers
			});
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			return fail(wrongPassword(known.message) ? 403 : known.status, {
				error: wrongPassword(known.message) ? 'Password is wrong.' : known.message
			});
		}
		await refreshAuthComplete(env, user.id);
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: '2fa.disable',
			outcome: 'ok'
		});
		return { disabled: true };
	},

	/** Fresh backup codes; the old ones stop working. */
	backupCodes: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const password = String((await request.formData()).get('password') || '');
		const hasPassword = (await linkedProviders(env, user.id)).includes('credential');
		let codes: string[] = [];
		try {
			const res = await locals.auth!.api.generateBackupCodes({
				body: { password: hasPassword ? password : undefined },
				headers: request.headers
			});
			codes = (res as { backupCodes?: string[] }).backupCodes ?? [];
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			return fail(wrongPassword(known.message) ? 403 : known.status, {
				error: wrongPassword(known.message) ? 'Password is wrong.' : known.message
			});
		}
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: '2fa.backup_codes',
			outcome: 'ok'
		});
		return { backupCodes: codes };
	},

	/** A new recovery key (replacing any earlier one), shown exactly once. */
	recoveryKey: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const key = await issueRecoveryKey(env, user.id);
		await refreshAuthComplete(env, user.id);
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: 'recovery.issue',
			outcome: 'ok'
		});
		return { recoveryKey: key };
	},

	recoveryKeyClear: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		await clearRecoveryKey(env, user.id);
		await refreshAuthComplete(env, user.id);
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: 'recovery.clear',
			outcome: 'ok'
		});
		return { recoveryKeyCleared: true };
	},

	revoke: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const id = str((await request.formData()).get('id'), 64);
		if (!id || id === locals.session?.id) return fail(400, { error: 'Bad session id.' });
		await revokeSession(env, user.id, id);
		await writeAudit(env, request, {
			actor: user,
			category: 'auth',
			action: 'session.revoke',
			outcome: 'ok',
			target: id.slice(0, 12)
		});
		return { revoked: true };
	},

	linkDiscord: async ({ request, locals }) => {
		const env = getEnv();
		requireUser(locals);
		if (!discordEnabled(env)) return fail(404, { error: 'Discord is not configured.' });
		const res = await locals.auth!.api.linkSocialAccount({
			body: { provider: 'discord', callbackURL: '/account?linked=discord' },
			headers: request.headers
		});
		if (!res.url) return fail(500, { error: 'Discord did not return an authorization URL.' });
		redirect(303, res.url);
	},

	unlinkDiscord: async ({ request, locals }) => unlink(request, locals, 'discord'),

	linkSteam: async (event) => {
		const env = getEnv();
		requireUser(event.locals);
		beginSteam(event, env, {
			mode: 'link',
			signup: false,
			next: '/account?linked=steam',
			back: '/account'
		});
	},

	unlinkSteam: async ({ request, locals }) => unlink(request, locals, 'steam'),

	/**
	 * Delete your own account. Password accounts confirm with the password; the rest type their
	 * username and must have signed in recently (Better Auth's session freshness check).
	 * The guards and the audit-trail scrub live in erasure.ts, on Better Auth's delete hooks.
	 */
	deleteAccount: async ({ request, locals }) => {
		const env = getEnv();
		const user = requireUser(locals);
		const form = await request.formData();
		const hasPassword = (await linkedProviders(env, user.id)).includes('credential');
		const password = String(form.get('password') || '');
		const confirm = str(form.get('confirm'), 32);
		if (hasPassword && !password) return fail(400, { error: 'Enter your password to confirm.' });
		if (!hasPassword && confirm.toLowerCase() !== user.username.toLowerCase())
			return fail(400, { error: 'Type your username to confirm.' });
		try {
			await locals.auth!.api.deleteUser({
				body: hasPassword ? { password } : {},
				headers: request.headers
			});
		} catch (err) {
			const known = normalizeError(err);
			if (!known) throw err;
			await writeAudit(env, request, {
				actor: user,
				category: 'user',
				action: 'account.delete',
				outcome: 'denied',
				message: known.message
			});
			if (wrongPassword(known.message)) return fail(403, { error: 'Password is wrong.' });
			if (/SESSION_EXPIRED|session expired/i.test(known.message))
				return fail(403, { error: 'Sign out, sign in again, then delete your account.' });
			return fail(known.status, { error: known.message });
		}
		await auditSelfDelete(env, request, user);
		redirect(303, '/sign-in?deleted=1');
	}
};

async function unlink(request: Request, locals: App.Locals, provider: 'discord' | 'steam') {
	const env = getEnv();
	const user = requireUser(locals);
	try {
		await unlinkProvider(env, user.id, provider);
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		return fail(known.status, { error: known.message });
	}
	await writeAudit(env, request, {
		actor: user,
		category: 'auth',
		action: 'account.unlink',
		outcome: 'ok',
		target: provider
	});
	return { unlinked: provider };
}
