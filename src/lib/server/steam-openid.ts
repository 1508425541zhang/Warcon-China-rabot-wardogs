// Steam sign-in. Steam speaks OpenID 2.0 (not OAuth), so this is the whole protocol: send the user
// to Steam with a return address, then hand Steam's signed answer straight back to Steam to check.
// No API key is involved; what comes back is the SteamID64 and nothing else.
import { createHmac, timingSafeEqual } from 'node:crypto';
import { ApiError } from './http';

export const STEAM_OPENID_URL = 'https://steamcommunity.com/openid/login';
const IDENTIFIER_SELECT = 'http://specs.openid.net/auth/2.0/identifier_select';
const NS = 'http://specs.openid.net/auth/2.0';

/** Where Steam sends the user to prove who they are. `returnTo` must sit under `realm`. */
export function steamLoginUrl(realm: string, returnTo: string): string {
	const q = new URLSearchParams({
		'openid.ns': NS,
		'openid.mode': 'checkid_setup',
		'openid.identity': IDENTIFIER_SELECT,
		'openid.claimed_id': IDENTIFIER_SELECT,
		'openid.return_to': returnTo,
		'openid.realm': realm
	});
	return `${STEAM_OPENID_URL}?${q}`;
}

export function steamIdFromClaimedId(claimedId: string | null | undefined): string | null {
	const m = /^https?:\/\/steamcommunity\.com\/openid\/id\/(\d{17})$/.exec(claimedId ?? '');
	return m ? m[1] : null;
}

/**
 * Validates the assertion Steam redirected back with. Structural checks first (mode, that the
 * answer is addressed to our callback, that the identity is a Steam profile), then the
 * check_authentication round trip, which is Steam confirming it signed exactly these fields.
 */
export async function verifySteamAssertion(
	params: URLSearchParams,
	expectedReturnTo: string,
	fetchFn: typeof fetch = fetch
): Promise<string> {
	// Every field once. get() reads the first of a repeated field and the body sent to Steam below
	// keeps the last, so a repeated identity would have Steam confirm one and this return another:
	// anyone could sign in as any Steam account by putting that identity in front of their own.
	const seen = new Set<string>();
	for (const [k] of params) {
		if (!k.startsWith('openid.')) continue;
		if (seen.has(k))
			throw new ApiError(403, 'Steam sign-in answer repeats a field.', 'steam_duplicate');
		seen.add(k);
	}
	if (params.get('openid.mode') !== 'id_res')
		throw new ApiError(403, 'Steam did not confirm the sign-in.', 'steam_denied');
	const returnTo = params.get('openid.return_to') ?? '';
	if (returnTo !== expectedReturnTo)
		throw new ApiError(403, 'Steam answered for a different request.', 'steam_return_to');
	const claimed = params.get('openid.claimed_id');
	const steamId = steamIdFromClaimedId(claimed);
	if (!steamId || params.get('openid.identity') !== claimed)
		throw new ApiError(403, 'Steam did not return a profile identity.', 'steam_identity');
	const signed = (params.get('openid.signed') ?? '').split(',');
	for (const must of ['claimed_id', 'identity', 'return_to', 'response_nonce', 'assoc_handle'])
		if (!signed.includes(must))
			throw new ApiError(403, 'Steam signature does not cover the identity.', 'steam_signed');

	const body = new URLSearchParams();
	for (const [k, v] of params) if (k.startsWith('openid.')) body.set(k, v);
	body.set('openid.mode', 'check_authentication');
	let text = '';
	try {
		const res = await fetchFn(STEAM_OPENID_URL, {
			method: 'POST',
			headers: { 'content-type': 'application/x-www-form-urlencoded' },
			body: body.toString(),
			signal: AbortSignal.timeout(10_000)
		});
		text = await res.text();
	} catch {
		throw new ApiError(
			502,
			'Steam could not be reached to confirm the sign-in.',
			'steam_unreachable'
		);
	}
	if (!/^is_valid\s*:\s*true\s*$/m.test(text))
		throw new ApiError(403, 'Steam rejected the sign-in signature.', 'steam_invalid');
	return steamId;
}

// ---- The round-trip state cookie -------------------------------------------------------------

export interface SteamState {
	/** random nonce, echoed in the return address so the answer is bound to this browser */
	nonce: string;
	/** sign in (creating the account if allowed) or link to the signed-in account */
	mode: 'signin' | 'link';
	/** may a Steam user without an account get one? (invite links and open sign-up) */
	signup: boolean;
	/** same-site path to land on afterwards */
	next: string;
	/** where to send the user with ?error=steam */
	back: string;
	exp: number;
}

const b64 = (s: string) => Buffer.from(s, 'utf8').toString('base64url');
const unb64 = (s: string) => Buffer.from(s, 'base64url').toString('utf8');
const mac = (secret: string, payload: string) =>
	createHmac('sha256', secret).update(payload).digest('base64url');

export function signSteamState(secret: string, state: SteamState): string {
	const payload = b64(JSON.stringify(state));
	return `${payload}.${mac(secret, payload)}`;
}

export function readSteamState(secret: string, value: string | undefined): SteamState | null {
	if (!value) return null;
	const dot = value.lastIndexOf('.');
	if (dot < 0) return null;
	const payload = value.slice(0, dot);
	const given = Buffer.from(value.slice(dot + 1));
	const want = Buffer.from(mac(secret, payload));
	if (given.length !== want.length || !timingSafeEqual(given, want)) return null;
	try {
		const state = JSON.parse(unb64(payload)) as SteamState;
		if (typeof state.nonce !== 'string' || state.exp < Date.now()) return null;
		return state;
	} catch {
		return null;
	}
}
