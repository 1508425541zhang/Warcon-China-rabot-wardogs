import { expect, test } from 'bun:test';
import {
	defaultGroupConfig,
	detectGroups,
	namePrefix,
	prefixSimilarity
} from './group-control-policy';
import type { Player } from './types';
const p = (name: string, id: number, faction: string | null = 'A'): Player => ({
	name,
	steamId: `7656119800000000${id}`,
	faction,
	kills: 0,
	deaths: 0,
	cash: 0,
	ping: null
});
test('normalizes full width, punctuation and Unicode; short names remain short', () => {
	expect(namePrefix('[ＡＢ] C_D foo', 4)).toBe('abcd');
	expect(namePrefix('黑-队★一起1', 4)).toBe('黑队一起');
	expect(namePrefix('😀a', 4)).toBe('a');
});
test('strict threshold, same faction, unique IDs and exact minimum size', () => {
	const players = [
		p('ABCD one', 1),
		p('[ABCX] two', 2),
		p('abcd other', 3, 'B'),
		p('AB', 4),
		p('ABCD', 5, null)
	];
	const config = { ...defaultGroupConfig, minPlayers: 2 };
	expect(prefixSimilarity('abcd', 'abcx')).toBe(75);
	expect(detectGroups(players, config, ['A', 'B'])).toHaveLength(1);
	expect(detectGroups(players, { ...config, similarityPercent: 75 }, ['A', 'B'])).toHaveLength(0);
	expect(detectGroups(players, { ...config, minPlayers: 3 }, ['A', 'B'])).toHaveLength(0);
	expect(detectGroups([players[0], players[0]], config, ['A'])).toHaveLength(0);
});
test('prevents similarity chaining and is stable across list ordering', () => {
	const players = [p('abcd', 1), p('abcx', 2), p('abxx', 3)];
	const config = { ...defaultGroupConfig, minPlayers: 2 };
	const groups = detectGroups(players, config, ['A']);
	expect(groups).toEqual(detectGroups([...players].reverse(), config, ['A']));
	expect(groups[0].members).toHaveLength(2);
	expect(groups[0].minimumSimilarity).toBe(75);
});
