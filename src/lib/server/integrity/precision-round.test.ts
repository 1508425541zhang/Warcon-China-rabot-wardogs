import { expect, test } from 'bun:test';
import { compareRoundPrecision } from './precision-round';
test('uses the whole supplied round, excludes self, merges category counts', () => {
	const [r] = compareRoundPrecision(
		[
			{ steamId: 'a', category: 'rifle', kills: 5, headshots: 3 },
			{ steamId: 'a', category: 'rifle', kills: 5, headshots: 4 },
			{ steamId: 'b', category: 'rifle', kills: 10, headshots: 2 },
			{ steamId: 'c', category: 'sniper', kills: 10, headshots: 10 }
		],
		'a'
	);
	expect(r.kills).toBe(10);
	expect(r.rate).toBe(0.7);
	expect(r.serverRate).toBe(0.2);
	expect(r.peerPlayers).toBe(1);
	expect(r.percentile).toBe(1);
});
test('equal rates are midrank, no peers is missing not zero', () => {
	const rows = [
		{ steamId: 'a', category: 'rifle', kills: 5, headshots: 3 },
		{ steamId: 'b', category: 'rifle', kills: 10, headshots: 6 }
	];
	expect(compareRoundPrecision(rows, 'a')[0].percentile).toBe(0.5);
	expect(compareRoundPrecision(rows.slice(0, 1), 'a')[0].serverRate).toBeNull();
});
test('invalid headshots cannot enter the population', () => {
	expect(
		compareRoundPrecision([{ steamId: 'a', category: 'rifle', kills: 5, headshots: 6 }], 'a')
	).toEqual([]);
});

import { precisionDecision } from './precision-round';
import { EXPERT_MODELS } from './committee';
import type { StatisticalAssessment } from './statistics';
test('inclusive class thresholds and five-kill minimum', () => {
	for (const [category, kills, headshots, decision] of [
		['automatic', 5, 3, 'SUSPICIOUS'],
		['automatic', 10, 7, 'CHEAT_LIKELY'],
		['automatic', 5, 5, 'CHEAT_LIKELY'],
		['automatic', 4, 4, 'UNKNOWN'],
		['sniper', 10, 7, 'SUSPICIOUS'],
		['sniper', 10, 9, 'CHEAT_LIKELY'],
		['shotgun', 10, 3, 'SUSPICIOUS'],
		['shotgun', 10, 5, 'CHEAT_LIKELY']
	] as const) {
		const row = compareRoundPrecision([{ steamId: 'a', category, kills, headshots }], 'a')[0];
		expect(precisionDecision(row)).toBe(decision);
	}
});
test('precision votes without any old 10-kill or 50-sample baseline', () => {
	const precision = compareRoundPrecision(
		[{ steamId: 'a', category: 'automatic', kills: 5, headshots: 5 }],
		'a'
	);
	const result = EXPERT_MODELS.find((m) => m.id === 'precision')!.assess({
		statistical: { metrics: [] } as unknown as StatisticalAssessment,
		precision,
		currentKpm: 1,
		eventIds: ['e'],
		independentEpisodes: 0
	});
	expect(result.decision).toBe('CHEAT_LIKELY');
	expect(result.modelVersion).toBe('2-round-class');
});
test('server percentile can vote below fixed threshold, ties cannot', () => {
	const peers = Array.from({ length: 20 }, (_, i) => ({
		steamId: `p${i}`,
		category: 'automatic',
		kills: 10,
		headshots: 1
	}));
	const own = { steamId: 'a', category: 'automatic', kills: 10, headshots: 2 };
	expect(precisionDecision(compareRoundPrecision([...peers, own], 'a')[0])).toBe('CHEAT_LIKELY');
	expect(
		precisionDecision(compareRoundPrecision([...peers, { ...own, headshots: 1 }], 'a')[0])
	).toBe('NORMAL');
});
