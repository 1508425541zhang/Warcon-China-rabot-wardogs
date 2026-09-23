import { describe, expect, test } from 'bun:test';
import {
	evaluateTriggers,
	MATCH_HOLD_MS,
	matchBroadcastStep,
	validateConfig,
	type MatchEnd,
	type TickContext
} from './triggers';
import type { TriggerRow } from './db/schema';
import type { Env } from './env';

const end: MatchEnd = {
	map: 'Bakurani',
	scores: [
		{ name: 'Valkyra', score: 100 },
		{ name: 'Lonestar', score: 81 }
	],
	winner: 'Valkyra',
	leaders: ['Valkyra']
};

describe('matchBroadcastStep', () => {
	test('an end seen with the server empty is held, then announced once players are back', () => {
		const a = matchBroadcastStep(null, end, [], 0, 1000, 1);
		expect(a.fire).toBeNull();
		expect(a.held?.end).toBe(end);
		const b = matchBroadcastStep(a.held, null, [], 0, 20_000, 1);
		expect(b.fire).toBeNull();
		const c = matchBroadcastStep(b.held, null, [], 45, 40_000, 1);
		expect(c.fire?.end).toBe(end);
		expect(c.held).toBeNull();
	});
	test('a scores reset then the map change keeps the result of the first', () => {
		const nil: MatchEnd = { ...end, scores: [], winner: null, leaders: [] };
		const a = matchBroadcastStep(null, end, [], 0, 1000, 1);
		const b = matchBroadcastStep(a.held, nil, [], 0, 5000, 1);
		expect(b.held?.end).toBe(end);
		expect(matchBroadcastStep(b.held, null, [], 60, 40_000, 1).fire?.end).toBe(end);
	});
	test('announced at once when the players are already on; dropped after the hold', () => {
		expect(matchBroadcastStep(null, end, [], 40, 1000, 1).fire?.end).toBe(end);
		const held = matchBroadcastStep(null, end, [], 0, 1000, 1).held;
		expect(matchBroadcastStep(held, null, [], 40, 1000 + MATCH_HOLD_MS + 1, 1)).toEqual({
			held: null,
			fire: null
		});
		expect(matchBroadcastStep(null, null, [], 40, 1000, 1)).toEqual({ held: null, fire: null });
	});
});

describe('the match broadcast rule', () => {
	const row = {
		id: `rule-${Math.random()}`,
		name: 'Match broadcast',
		kind: 'match_broadcast',
		config: validateConfig('match_broadcast', {
			endMessage: 'Fin du match: {faction} gagne sur {previous} · {scores}',
			startMessage: 'Prochain match sur {map}. Bon jeu!',
			minPlayers: 1
		})
	} as unknown as TriggerRow;
	const tick = (playerCount: number, matchEnd: MatchEnd | null, ts: number) =>
		({
			server: { id: 'srv', name: 'Server' },
			status: { playerCount, maxPlayers: 100, map: 'Madrid', scores: [] },
			players: [],
			joined: [],
			renamed: [],
			returned: [],
			riskCheck: [],
			matchEnd,
			matchLines: [],
			ts: new Date(ts)
		}) as unknown as TickContext;

	const evaluate = async (ctx: TickContext, committed = true) => {
		const ev = await evaluateTriggers({} as Env, ctx, [row]);
		if (committed) for (const f of ev.afterCommit ?? []) f();
		return ev;
	};

	test('a match that ends while the next map loads (nobody on) is announced when players are back', async () => {
		// the write of the boundary look fails: the end is held all the same
		const first = await evaluate(tick(0, end, 1000), false);
		expect(first.intents).toHaveLength(0);
		// the write of the announcing look fails: the end is still held for the next look
		const lost = await evaluate(tick(63, null, 34_000), false);
		expect(lost.intents).toHaveLength(2);
		const later = await evaluate(tick(63, null, 36_000));
		expect(later.intents.map((i) => i.params.message)).toEqual([
			'Fin du match: Valkyra gagne sur Bakurani · Valkyra 100 · Lonestar 81',
			'Prochain match sur Madrid. Bon jeu!'
		]);
		// once only
		const again = await evaluate(tick(63, null, 40_000));
		expect(again.intents).toHaveLength(0);
	});
});
