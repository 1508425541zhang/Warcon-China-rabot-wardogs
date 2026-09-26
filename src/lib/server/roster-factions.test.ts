import { expect, test } from 'bun:test';
import { rosterFactions } from './roster-factions';

test('live display names and feed catalog IDs identify the same map', () => {
	const at = new Date();
	const live = {
		playersAt: at,
		statusAt: at,
		status: { map: 'Ozeti', scores: [{ name: 'Lonestar' }, { name: 'Valkyra' }] },
		players: [{ steamId: '76561198000000101', faction: 'Lonestar' }]
	};
	expect(rosterFactions(live, 'Europe')?.get('76561198000000101')).toBe('Lonestar');
	expect(rosterFactions(live, 'Kavkazi')).toBeNull();
	expect(
		rosterFactions({ ...live, playersAt: new Date(at.getTime() + 11_000) }, 'Europe')
	).toBeNull();
});
