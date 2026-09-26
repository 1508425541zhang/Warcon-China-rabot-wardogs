import { expect, test } from 'bun:test';
import { currentRisk } from './current-risk';
import { DEFAULT_INTEGRITY_RULES as config } from './score';
const known = { known: true, vacBans: 1, gameBans: 0, daysSinceLastBan: 30 };
test('VAC scores without a kill or saved assessment', () => {
	expect(currentRisk(undefined, known, 0, config)?.score).toBe(8);
	expect(currentRisk(undefined, { ...known, daysSinceLastBan: 400 }, 0, config)?.score).toBe(1);
});
test('unknown is not clean, and reports do not need Steam or KPM', () => {
	const unknown = { known: false, vacBans: 0, gameBans: 0, daysSinceLastBan: null };
	expect(currentRisk(undefined, unknown, 0, config)).toBeNull();
	expect(currentRisk(undefined, { ...unknown, known: true }, 0, config)?.score).toBe(0);
	expect(currentRisk(undefined, unknown, 3, config)?.score).toBe(4);
});
test('replace cached priors and reporter points without double counting behavior', () => {
	const saved = {
		score: 31,
		level: 'PASSIVE_WATCH',
		breakdown: [
			{ code: 'infantry_kpm_180', points: 18, detail: '' },
			{ code: 'steam_ban_prior', points: 12, detail: '' },
			{ code: 'unique_reports', points: 1, detail: '' }
		]
	};
	expect(currentRisk(saved, known, 3, config)?.score).toBe(30);
	expect(
		currentRisk(saved, { known: false, vacBans: 0, gameBans: 0, daysSinceLastBan: null }, 0, config)
			?.score
	).toBe(18);
});
