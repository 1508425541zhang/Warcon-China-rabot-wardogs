import { describe, expect, test } from 'bun:test';
import {
	readSteamState,
	signSteamState,
	steamIdFromClaimedId,
	steamLoginUrl,
	verifySteamAssertion
} from './steam-openid';

const RETURN = 'https://panel.example/auth/steam/callback?state=abc';
const good = () =>
	new URLSearchParams({
		'openid.ns': 'http://specs.openid.net/auth/2.0',
		'openid.mode': 'id_res',
		'openid.op_endpoint': 'https://steamcommunity.com/openid/login',
		'openid.claimed_id': 'https://steamcommunity.com/openid/id/76561198000000001',
		'openid.identity': 'https://steamcommunity.com/openid/id/76561198000000001',
		'openid.return_to': RETURN,
		'openid.response_nonce': '2026-09-15T00:00:00Zabc',
		'openid.assoc_handle': '1234567890',
		'openid.signed': 'signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle',
		'openid.sig': 'base64sig='
	});

describe('steam openid', () => {
	test('login url targets identifier_select under our realm', () => {
		const u = new URL(steamLoginUrl('https://panel.example', RETURN));
		expect(u.origin + u.pathname).toBe('https://steamcommunity.com/openid/login');
		expect(u.searchParams.get('openid.return_to')).toBe(RETURN);
		expect(u.searchParams.get('openid.realm')).toBe('https://panel.example');
	});
	test('claimed id parsing', () => {
		expect(steamIdFromClaimedId('https://steamcommunity.com/openid/id/76561198000000001')).toBe(
			'76561198000000001'
		);
		expect(steamIdFromClaimedId('https://evil.example/openid/id/76561198000000001')).toBeNull();
		expect(steamIdFromClaimedId('https://steamcommunity.com/openid/id/123')).toBeNull();
	});
	test('a valid assertion is confirmed with Steam and yields the id', async () => {
		let posted: URLSearchParams | null = null;
		const fetchFn = (async (_url: string, init: RequestInit) => {
			posted = new URLSearchParams(String(init.body));
			return new Response('ns:http://specs.openid.net/auth/2.0\nis_valid:true\n');
		}) as unknown as typeof fetch;
		expect(await verifySteamAssertion(good(), RETURN, fetchFn)).toBe('76561198000000001');
		expect(posted!.get('openid.mode')).toBe('check_authentication');
		expect(posted!.get('openid.sig')).toBe('base64sig=');
	});
	// get() reads the first of a repeated field and the body sent to Steam kept the last, so the
	// attacker's own signed fields were confirmed while the victim's identity in front was returned.
	test('an assertion that repeats a field is refused before Steam is asked', async () => {
		let asked = 0;
		const fetchFn = (async () => {
			asked++;
			return new Response('is_valid:true\n');
		}) as unknown as typeof fetch;
		const VICTIM = 'https://steamcommunity.com/openid/id/76561198000000099';

		// The attack itself: the victim's identity in front of the attacker's own signed answer.
		const forged = new URLSearchParams([
			['openid.claimed_id', VICTIM],
			['openid.identity', VICTIM],
			...good()
		]);
		await expect(verifySteamAssertion(forged, RETURN, fetchFn)).rejects.toMatchObject({
			code: 'steam_duplicate'
		});

		// And every field, repeated in front or behind, with the same value or another.
		for (const [field, value] of good())
			for (const extra of [value, 'something-else'])
				for (const where of ['front', 'behind'] as const) {
					const twice = new URLSearchParams(
						where === 'front' ? [[field, extra], ...good()] : [...good(), [field, extra]]
					);
					await expect(verifySteamAssertion(twice, RETURN, fetchFn)).rejects.toMatchObject({
						code: 'steam_duplicate'
					});
				}
		expect(asked).toBe(0);
	});
	test('a repeated parameter that is not part of the assertion does not matter', async () => {
		const fetchFn = (async () => new Response('is_valid:true\n')) as unknown as typeof fetch;
		const params = new URLSearchParams([['utm', 'a'], ...good(), ['utm', 'b']]);
		expect(await verifySteamAssertion(params, RETURN, fetchFn)).toBe('76561198000000001');
	});
	test('steam saying is_valid:false is refused', async () => {
		const fetchFn = (async () => new Response('is_valid:false\n')) as unknown as typeof fetch;
		await expect(verifySteamAssertion(good(), RETURN, fetchFn)).rejects.toMatchObject({
			code: 'steam_invalid'
		});
	});
	test('a mismatched return address or identity never reaches Steam', async () => {
		const fetchFn = (async () => {
			throw new Error('should not be called');
		}) as unknown as typeof fetch;
		await expect(verifySteamAssertion(good(), RETURN + 'x', fetchFn)).rejects.toMatchObject({
			code: 'steam_return_to'
		});
		const p = good();
		p.set('openid.identity', 'https://steamcommunity.com/openid/id/76561198000000002');
		await expect(verifySteamAssertion(p, RETURN, fetchFn)).rejects.toMatchObject({
			code: 'steam_identity'
		});
		const q = good();
		q.set('openid.signed', 'signed,op_endpoint');
		await expect(verifySteamAssertion(q, RETURN, fetchFn)).rejects.toMatchObject({
			code: 'steam_signed'
		});
	});
	test('state cookie round trip and tamper detection', () => {
		const state = {
			nonce: 'n1',
			mode: 'signin' as const,
			signup: true,
			next: '/',
			back: '/sign-in',
			exp: Date.now() + 60_000
		};
		const v = signSteamState('secret', state);
		expect(readSteamState('secret', v)).toEqual(state);
		expect(readSteamState('other', v)).toBeNull();
		expect(readSteamState('secret', v.slice(0, -2) + 'zz')).toBeNull();
		expect(readSteamState('secret', signSteamState('secret', { ...state, exp: 1 }))).toBeNull();
	});
});
