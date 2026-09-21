import { describe, expect, test } from 'bun:test';
import { evaluateTriggers, type TickContext } from './triggers';
import type { TriggerRow } from './db/schema';
import { validateNameFilter } from './name-filter';
import type { Env } from './env';
import type { Player } from '$lib/types';

const player = (steamId: string, name: string): Player => ({
	name,
	steamId,
	faction: null,
	kills: 0,
	deaths: 0,
	cash: 0,
	ping: null
});
const row = (config: Record<string, unknown>) =>
	({
		id: 'rule',
		kind: 'name_filter',
		config: validateNameFilter(config)
	}) as unknown as TriggerRow;
const tick = (joined: Player[], renamed: Player[], reserved: string[] = []) =>
	({
		server: { id: 'srv', name: 'Server' },
		status: { players: joined.length + renamed.length, maxPlayers: 64 },
		players: [...joined, ...renamed],
		joined,
		renamed,
		factioned: [],
		firstVisit: new Set(),
		reserved: new Set(reserved),
		ts: new Date(0)
	}) as unknown as TickContext;
const run = (config: Record<string, unknown>, ctx: TickContext) =>
	evaluateTriggers({} as Env, ctx, [row(config)]);

describe('the name filter rule', () => {
	test('judges a name that arrives after the join: a clan tag shown a look later', async () => {
		const ev = await run(
			{ blocked: ['tag'] },
			tick([player('1', 'Joiner')], [player('2', '[TAG] Player'), player('3', '[GG] Fine')])
		);
		expect(ev.intents.map((i) => [i.action, i.target])).toEqual([['kick', '2']]);
		expect(ev.intents[0].detail).toEqual({ name: '[TAG] Player', verdict: "blocked word 'tag'" });
	});
	test('still judges joiners, and spares a reserved slot on a rename too', async () => {
		const ev = await run(
			{ blocked: ['tag'] },
			tick([player('1', '[TAG] Joiner')], [player('2', '[TAG] Player')], ['2'])
		);
		expect(ev.intents.map((i) => i.target)).toEqual(['1']);
	});
	test('a look with no joiner and no new name does nothing', async () => {
		const ev = await run({ blocked: ['tag'] }, tick([], []));
		expect(ev).toEqual({ intents: [], updates: [] });
	});
});
