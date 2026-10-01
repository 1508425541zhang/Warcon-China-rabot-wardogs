import { expect, test } from 'bun:test';
import { battleMessage, factionBlock } from './battle';

const status = {
	serverName: 'OPEN1',
	map: 'Kavkazi',
	matchSeconds: 125,
	scoreCap: 100,
	scores: [
		{ name: 'RED', colorHex: '#D86060', score: 54 },
		{ name: 'BLU', colorHex: '#5B95D8', score: 27 },
		{ name: 'GRN', colorHex: '#7BC462', score: 100 }
	]
};

test('victory progress uses the target rather than relative share, with actual team counts', () => {
	const text = battleMessage(status, [{ name: 'Alice', steamId: '1', faction: 'RED' }]);
	expect(text).toContain('🟥 RED · 1人 · 54 / 100');
	expect(text).toContain('🟥'.repeat(5) + '▫️'.repeat(5) + ' 54%');
	expect(text).toContain('🟩'.repeat(10) + ' 100%');
	expect(text).toContain('2分5秒');
	expect(text).toContain('🟥 Alice · RED');
});

test('missing target and score are never represented as zero progress', () => {
	const text = battleMessage(
		{
			...status,
			scoreCap: null,
			matchSeconds: null,
			scores: [{ name: 'RED', colorHex: '', score: NaN }]
		},
		[]
	);
	expect(text).toContain('未知 / 未知');
	expect(text).toContain('胜利进度未知');
	expect(text).not.toContain(' 0%');
	expect(text).toContain('当前无人在线');
});

test('every player including unassigned players is reachable without cutting the QQ reply', () => {
	const players = Array.from({ length: 100 }, (_, i) => ({
		name: `player-${i}`,
		steamId: String(i),
		faction: i === 99 ? null : ['RED', 'BLU', 'GRN'][i % 3]
	}));
	const all = [1, 2, 3, 4].map((p) => battleMessage(status, players, p)).join('\n');
	for (const player of players)
		expect(all.split('\n').filter((line) => line.includes(` ${player.name} ·`))).toHaveLength(1);
	expect(all).toContain('⬜ player-99 · 未入阵营');
	expect(() => battleMessage(status, players, 5)).toThrow('共有 4 页');
	const long = players.map((p) => ({ ...p, name: '🟥'.repeat(100) }));
	expect(battleMessage(status, long).length).toBeLessThan(3500);
});

test('actual game colors determine the marker; no invented side for neutral factions', () => {
	expect(factionBlock('Valkyra', '#D86060')).toBe('🟥');
	expect(factionBlock('Lonestar', '#5B95D8')).toBe('🟦');
	expect(factionBlock('Manticore', '#7BC462')).toBe('🟩');
	expect(factionBlock('White', '#FFFFFF')).toBe('⬜');
	expect(battleMessage({ ...status, scores: [{ ...status.scores[0], score: 150 }] }, [])).toContain(
		'100%'
	);
});
