import { describe, expect, test } from 'bun:test';
import { evaluateTriggers, riskCheckTargets, type TickContext } from './triggers';
import { validateConfig } from './trigger-rules';
import type { TriggerRow } from './db/schema';
import type { Env } from './env';
import type { Player } from '$lib/types';

const player = (steamId: string): Player => ({
	name: `p${steamId.slice(-2)}`,
	steamId,
	faction: null,
	kills: 0,
	deaths: 0,
	cash: 0,
	ping: null
});
const A = player('76561198000000401');
const B = player('76561198000000402');
const C = player('76561198000000403');

describe('riskCheckTargets', () => {
	test('joiners and returned players are judged at every look, everyone else at a sweep', () => {
		const ids = (ps: Player[]) => ps.map((p) => p.steamId);
		expect(ids(riskCheckTargets([A], [B], [B, C], false))).toEqual([A.steamId, B.steamId]);
		expect(ids(riskCheckTargets([A], [B], [B, C], true))).toEqual([
			A.steamId,
			B.steamId,
			C.steamId
		]);
		expect(riskCheckTargets([], [], [B, C], false)).toEqual([]);
	});
});

describe('the risk kick rule', () => {
	const row = {
		id: 'rule',
		name: 'Kick on connect risk',
		kind: 'risk_kick',
		config: validateConfig('risk_kick', { watchlist: true, reason: 'no' })
	} as unknown as TriggerRow;
	const tick = (joined: Player[], riskCheck: Player[]) =>
		({
			server: { id: 'srv', name: 'Server' },
			joined,
			renamed: [],
			returned: [],
			riskCheck,
			reserved: new Set(),
			signals: new Map(
				[A, B, C].map((p) => [p.steamId, { watched: { reason: 'x' }, bannedOn: [], resembles: [] }])
			),
			profiles: new Map(),
			performance: new Map(),
			ts: new Date(0)
		}) as unknown as TickContext;

	test('kicks a matching player who is on the server, not only one who has just joined', async () => {
		// B has no join at this look (a reconnect inside the grace, or on before the rule existed)
		const ev = await evaluateTriggers({} as Env, tick([], [B]), [row]);
		expect(ev.intents.map((i) => [i.action, i.target])).toEqual([['kick', B.steamId]]);
		expect(ev.updates[0].lastResult).toBe(`Kicking ${B.name}: on the watchlist (x)`);
	});

	test('judges nobody when nobody is due', async () => {
		const ev = await evaluateTriggers({} as Env, tick([A], []), [row]);
		expect(ev).toEqual({ intents: [], updates: [] });
	});
});
