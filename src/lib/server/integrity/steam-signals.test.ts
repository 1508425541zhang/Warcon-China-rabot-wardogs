import { expect, test } from 'bun:test';
import { steamBanSignals } from './steam-signals';
import { scoreIntegrity, type IntegritySignals } from './score';

const now = new Date('2026-01-01T00:00:00Z');
const base: IntegritySignals = {
	behaviorReasons: ['burst'],
	kpm180: 8 / 3,
	uniqueVictims: 8,
	previousKpm: [],
	uniqueReporters: 0,
	repeatHighRiskWindow: false,
	infantryKills: 8,
	headshots: 0,
	penetrations: 0,
	burstPoints: 12,
	vacBans: 0,
	gameBans: 0,
	daysSinceLastBan: null,
	wardogsPlaytimeHours: null
};

test('cached VAC history contributes to the Integrity breakdown, independent of privacy', () => {
	const profile = {
		fetchedAt: now,
		error: '',
		vacBans: 1,
		gameBans: 0,
		daysSinceLastBan: 10,
		public: false
	};
	const steam = steamBanSignals(profile, now);
	expect(steam.known).toBe(true);
	expect(
		scoreIntegrity({ ...base, ...steam }).breakdown.find((item) => item.code === 'steam_ban_prior')
			?.points
	).toBe(8);
	const publicRow = { ...profile, public: true };
	const publicProfile = steamBanSignals(publicRow, now);
	expect(publicProfile).toEqual(steam);
});

test('unavailable, errored and stale Steam data are UNKNOWN and add no risk', () => {
	const valid = { fetchedAt: now, error: '', vacBans: 1, gameBans: 1, daysSinceLastBan: 10 };
	for (const profile of [
		undefined,
		{ ...valid, error: 'Steam failed' },
		{ ...valid, fetchedAt: new Date(now.getTime() - 25 * 3600_000) }
	]) {
		const steam = steamBanSignals(profile, now);
		expect(steam).toEqual({ known: false, vacBans: 0, gameBans: 0, daysSinceLastBan: null });
		expect(
			scoreIntegrity({ ...base, ...steam }).breakdown.some(
				(item) => item.code === 'steam_ban_prior'
			)
		).toBe(false);
	}
});
