// Browser side of passkeys: the WebAuthn ceremonies, talking to the panel's own /api/passkeys
// routes (which relay to Better Auth server-side, so the auth API itself stays closed).
import {
	browserSupportsWebAuthn,
	startAuthentication,
	startRegistration
} from '@simplewebauthn/browser';
import { api, ApiError } from './api';

export const passkeysSupported = (): boolean =>
	typeof window !== 'undefined' && browserSupportsWebAuthn();

/** Details for a passkey-first sign-up; omitted when a signed-in user adds a passkey. */
export interface PasskeySignupInput {
	username: string;
	displayName?: string;
	/** first-run owner setup token, when the panel requires one */
	token?: string;
	/** invite link token, when sign-up is by invitation */
	invite?: string;
	turnstile?: string;
}

function friendly(err: unknown): never {
	if (err instanceof ApiError) throw err;
	const name = (err as { name?: string })?.name ?? '';
	if (name === 'NotAllowedError')
		throw new ApiError('Cancelled or timed out. Try again.', 0, 'webauthn_cancelled');
	if (name === 'InvalidStateError')
		throw new ApiError(
			'This device already holds a passkey for this account.',
			0,
			'webauthn_duplicate'
		);
	if (name === 'SecurityError')
		throw new ApiError(
			'Passkeys need a secure (https) origin that matches the panel address.',
			0,
			'webauthn_origin'
		);
	throw new ApiError(
		(err as Error)?.message || 'Your browser could not complete the passkey request.',
		0,
		'webauthn'
	);
}

/** Registers a passkey: for the signed-in account, or (with `signup`) for a brand-new one. */
export async function addPasskey(
	name: string,
	signup?: PasskeySignupInput
): Promise<{ id: string; userId: string }> {
	const options = await api('POST', '/api/passkeys/register-options', { name, ...(signup ?? {}) });
	let response: unknown;
	try {
		response = await startRegistration({ optionsJSON: options });
	} catch (err) {
		friendly(err);
	}
	return api('POST', '/api/passkeys', { response, name });
}

/** The sign-in ceremony; on success the session cookie is set and the caller navigates. */
export async function signInWithPasskey(): Promise<void> {
	const options = await api('POST', '/api/passkeys/auth-options');
	let response: unknown;
	try {
		response = await startAuthentication({ optionsJSON: options });
	} catch (err) {
		friendly(err);
	}
	await api('POST', '/api/passkeys/auth', { response });
}

/** A default label from the browser, so "Add passkey" needs no typing. */
export function suggestPasskeyName(): string {
	if (typeof navigator === 'undefined') return 'Passkey';
	const ua = navigator.userAgent;
	if (/iPhone|iPad/.test(ua)) return 'iPhone';
	if (/Android/.test(ua)) return 'Android phone';
	if (/Macintosh/.test(ua)) return 'Mac';
	if (/Windows/.test(ua)) return 'Windows PC';
	if (/Linux/.test(ua)) return 'Linux PC';
	return 'Passkey';
}
