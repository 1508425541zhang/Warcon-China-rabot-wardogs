// The Steam sign-in round trip for pages: begin() stamps a signed state cookie and sends the user
// to Steam; the callback route (routes/auth/steam/callback) reads it back and finishes.
import { redirect, type Cookies, type RequestEvent } from '@sveltejs/kit';
import { randomBytes } from 'node:crypto';
import type { Env } from './env';
import { readSteamState, signSteamState, steamLoginUrl, type SteamState } from './steam-openid';

export const STEAM_STATE_COOKIE = 'warcon.steam';
const STATE_MAX_AGE = 10 * 60;

export const steamCallbackUrl = (env: Pick<Env, 'ORIGIN'>, nonce: string) =>
	`${env.ORIGIN}/auth/steam/callback?state=${nonce}`;

/** Sets the state cookie and redirects to Steam. Never returns. */
export function beginSteam(
	event: Pick<RequestEvent, 'cookies'>,
	env: Env,
	opts: Omit<SteamState, 'nonce' | 'exp'>
): never {
	const nonce = randomBytes(16).toString('base64url');
	const state: SteamState = { ...opts, nonce, exp: Date.now() + STATE_MAX_AGE * 1000 };
	event.cookies.set(STEAM_STATE_COOKIE, signSteamState(env.BETTER_AUTH_SECRET!, state), {
		path: '/auth/steam',
		httpOnly: true,
		sameSite: 'lax',
		secure: env.ORIGIN.startsWith('https://'),
		maxAge: STATE_MAX_AGE
	});
	redirect(303, steamLoginUrl(env.ORIGIN, steamCallbackUrl(env, nonce)));
}

/** Reads and clears the state cookie; null when missing, tampered, expired or for another nonce. */
export function takeSteamState(
	cookies: Cookies,
	env: Env,
	nonce: string | null
): SteamState | null {
	const state = readSteamState(env.BETTER_AUTH_SECRET!, cookies.get(STEAM_STATE_COOKIE));
	cookies.delete(STEAM_STATE_COOKIE, { path: '/auth/steam' });
	if (!state || !nonce || state.nonce !== nonce) return null;
	return state;
}
