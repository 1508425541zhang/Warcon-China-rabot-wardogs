// The sign-in rules, free of any database so they can be unit-tested and shared with the browser.
//
// The panel holds no email address, so nobody can be sent a reset link. Instead every account must
// be able to survive losing one thing: two independent ways in, a second factor on any password,
// and for owners (whom nobody can reset) a linked sign-in provider or a saved recovery key.

/** Sign-in providers that count as a way in on their own (the provider holds the identity). */
export const SSO_PROVIDERS = ['discord', 'steam'] as const;
export type SsoProvider = (typeof SSO_PROVIDERS)[number];

export interface AuthMethods {
	/** a credential account exists (username + password) */
	password: boolean;
	/** an authenticator app is enrolled and verified */
	twoFactor: boolean;
	/** WebAuthn credentials on file */
	passkeys: number;
	/** linked account providers, e.g. ['credential', 'discord'] */
	providers: string[];
	/** a recovery key was issued and has not been used */
	recoveryKey: boolean;
}

export interface Enrolment {
	/** independent sign-in paths: passkeys, linked providers, password+2FA, recovery key */
	waysIn: number;
	complete: boolean;
	/** what is missing, in the order it should be fixed; empty when complete */
	problems: string[];
}

export const ssoProviders = (providers: string[]): SsoProvider[] =>
	SSO_PROVIDERS.filter((p) => providers.includes(p));

export function assessEnrolment(m: AuthMethods, role: 'owner' | 'member'): Enrolment {
	const sso = ssoProviders(m.providers);
	const strongPassword = m.password && m.twoFactor;
	const waysIn = m.passkeys + sso.length + (strongPassword ? 1 : 0) + (m.recoveryKey ? 1 : 0);
	const problems: string[] = [];
	if (m.password && !m.twoFactor)
		problems.push(
			'Your password has no second factor. Turn on an authenticator app, or add a passkey and remove the password.'
		);
	if (waysIn < 2)
		problems.push(
			waysIn === 0
				? 'You have no safe way in yet. Add a passkey or link Discord or Steam, then add a second one.'
				: 'You have only one way in. Add a second: another passkey, a linked Discord or Steam account, or a recovery key.'
		);
	if (role === 'owner' && !sso.length && !m.recoveryKey)
		problems.push(
			'Owners cannot be reset by anyone else, so link Discord or Steam or save a recovery key.'
		);
	return { waysIn, complete: problems.length === 0, problems };
}

export interface GraceSettings {
	/** days an owner may sign in before the rules are enforced */
	authGraceDays: number;
	/** the same for members */
	authMemberGraceDays: number;
}

export interface EnrolmentSubject {
	role: 'owner' | 'member';
	authComplete: boolean;
	/** ISO time of the first sign-in since the rules arrived; null until then */
	authGraceStartedAt: string | null;
}

export interface EnrolmentStatus {
	/** the account fails the rules and its grace period is over: the panel is closed until fixed */
	due: boolean;
	/** whole days left before `due`, null when complete or when the grace period has not started */
	daysLeft: number | null;
	deadline: string | null;
}

export function enrolmentStatus(
	u: EnrolmentSubject,
	s: GraceSettings,
	now = Date.now()
): EnrolmentStatus {
	if (u.authComplete || !u.authGraceStartedAt)
		return { due: false, daysLeft: null, deadline: null };
	const days = u.role === 'owner' ? s.authGraceDays : s.authMemberGraceDays;
	const deadline = new Date(u.authGraceStartedAt).getTime() + days * 86_400_000;
	const left = deadline - now;
	return {
		due: left <= 0,
		daysLeft: Math.max(0, Math.ceil(left / 86_400_000)),
		deadline: new Date(deadline).toISOString()
	};
}
