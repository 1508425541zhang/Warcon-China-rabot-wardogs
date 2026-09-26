import { expect, test } from 'bun:test';
import { awardWinners, defaultAwards, type AwardLine } from './match-awards-policy';
import { matchBroadcastMessages, validateConfig } from './server/trigger-rules';
const lines: AwardLine[] = [
	{
		steamId: 'a',
		name: '甲',
		kills: 10,
		deaths: 2,
		cashDelta: 600,
		cashHeld: 200,
		seconds: 600,
		multi: 3,
		streak: 6,
		awardSeed: 'round1'
	},
	{
		steamId: 'b',
		name: '乙',
		kills: 6,
		deaths: 0,
		cashDelta: 100,
		cashHeld: 800,
		seconds: 900,
		multi: 2,
		streak: 5,
		awardSeed: 'round1'
	}
];
test('six rankings distinguish net earnings and final cash; zero death uses divisor one', () => {
	const winners = awardWinners(lines);
	expect(Object.fromEntries(winners.map((w) => [w.key, w.player.steamId]))).toEqual({
		mvp: 'b',
		multi: 'a',
		streak: 'a',
		earned: 'a',
		rich: 'b',
		time: 'b'
	});
	expect(winners.find((w) => w.key === 'mvp')?.value).toBe('6.00');
});
test('single winner for ties, stable across retries and source ordering, unknowns omitted', () => {
	const tied = [
		{ ...lines[0], steamId: '1' },
		{ ...lines[0], steamId: '2' },
		{ ...lines[0], steamId: '3' }
	];
	expect(awardWinners(tied)).toHaveLength(6);
	expect(awardWinners(tied)).toEqual(awardWinners([...tied].reverse()));
	expect(awardWinners([{ name: '空', kills: 0, deaths: 0 }])).toHaveLength(0);
	expect(awardWinners([{ name: '单杀', kills: 1, multi: 1 }])).toHaveLength(0);
});
test('awards opt in; separate bounded broadcasts, template placeholders and min players', () => {
	const end = { map: 'old', scores: [{ name: 'A', score: 100 }], winner: 'A', leaders: ['A'] };
	const config = {
		endMessage: '',
		startMessage: '',
		minPlayers: 1,
		awards: { ...defaultAwards, enabled: true }
	};
	expect(() => validateConfig('match_broadcast', config)).not.toThrow();
	const messages = matchBroadcastMessages(config, end, 10, {}, lines);
	expect(messages).toHaveLength(6);
	expect(messages[0].message).toContain('乙');
	expect(messages.every((m) => m.message.length <= 200 && !m.message.includes('{name}'))).toBe(
		true
	);
	expect(matchBroadcastMessages(config, end, 0, {}, lines)).toHaveLength(0);
	expect(
		matchBroadcastMessages(
			{ ...config, awards: { ...defaultAwards, enabled: false } },
			end,
			10,
			{},
			lines
		)
	).toHaveLength(0);
});
