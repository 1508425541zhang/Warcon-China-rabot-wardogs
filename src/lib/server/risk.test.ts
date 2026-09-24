import { describe, expect, test } from 'bun:test';
import { accountAgeDays, assessRisk, levenshtein, namesResemble, normaliseName } from './risk';

const now = new Date('2026-09-09T12:00:00Z');
const clean = {
	public: true,
	accountCreatedAt: new Date('2020-01-01T00:00:00Z'),
	vacBans: 0,
	gameBans: 0,
	daysSinceLastBan: null,
	communityBanned: false,
	economyBan: 'none'
};
const base = { steamEnabled: true, watched: null, bannedOn: [], resembles: [], now };

describe('assessRisk', () => {
	test('an old clean account with nothing local is low', () => {
		const r = assessRisk({ ...base, profile: clean });
		expect(r.level).toBe('low');
		expect(r.score).toBe(0);
		expect(r.steamChecked).toBe(true);
	});
	test('a missing Steam profile is unavailable, not private', () => {
		const r = assessRisk({
			...base,
			profile: { ...clean, public: false, accountCreatedAt: null, error: 'Not found on Steam.' }
		});
		expect(r.score).toBe(0);
		expect(r.steamChecked).toBe(false);
	});
	test('a recent VAC ban alone is high', () => {
		const r = assessRisk({ ...base, profile: { ...clean, vacBans: 1, daysSinceLastBan: 40 } });
		expect(r.level).toBe('high');
		expect(r.reasons.map((x) => x.code)).toEqual(['vac']);
	});
	test('the same ban loses weight as it ages', () => {
		const scores = [7, 200, 700, 2000].map(
			(daysSinceLastBan) =>
				assessRisk({ ...base, profile: { ...clean, vacBans: 1, daysSinceLastBan } }).score
		);
		expect(scores).toEqual([60, 53, 45, 30]);
	});
	test('banned friends and private friends add bounded, explainable evidence', () => {
		const publicFriends = assessRisk({
			...base,
			profile: {
				...clean,
				friendsState: 'partial',
				friendsTotal: 320,
				friendsChecked: 200,
				bannedFriends: 3
			}
		});
		expect(publicFriends.reasons[0].code).toBe('banned_friends');
		expect(publicFriends.reasons[0].text).toContain('among 200 checked of 320');
		expect(publicFriends.score).toBe(21);
		const privateFriends = assessRisk({ ...base, profile: { ...clean, friendsState: 'private' } });
		expect(privateFriends.reasons[0].code).toBe('friends_private');
		expect(privateFriends.score).toBe(8);
	});
	test('performance needs enough recorded games or kills and cannot exceed 100', () => {
		const performance = {
			matches: 30,
			wins: 26,
			losses: 4,
			draws: 0,
			kills: 200,
			deaths: 30,
			feedKills: 100,
			headshots: 70
		};
		const risk = assessRisk({ ...base, profile: clean, performance });
		expect(risk.reasons.map((r) => r.code)).toEqual(['win_rate', 'kd', 'headshots']);
		expect(risk.score).toBe(28);
		const sparse = assessRisk({
			...base,
			profile: clean,
			performance: {
				...performance,
				wins: 2,
				matches: 2,
				losses: 0,
				kills: 10,
				deaths: 1,
				feedKills: 5,
				headshots: 5
			}
		});
		expect(sparse.score).toBe(0);
		expect(
			assessRisk({
				...base,
				profile: { ...clean, vacBans: 10, gameBans: 10, bannedFriends: 10 },
				performance
			}).score
		).toBe(100);
	});
	test('a week-old private account stacks age and privacy', () => {
		const r = assessRisk({
			...base,
			profile: { ...clean, public: false, accountCreatedAt: null }
		});
		expect(r.reasons.map((x) => x.code)).toEqual(['private']);
		expect(assessRisk({ ...base, profile: { ...clean, public: false } }).score).toBe(12);
		const young = assessRisk({
			...base,
			profile: { ...clean, accountCreatedAt: new Date('2026-09-07T00:00:00Z') }
		});
		expect(young.reasons[0].text).toBe('Steam account is 2 days old');
		expect(young.level).toBe('medium');
		expect(assessRisk({ ...base, profile: { ...clean, accountCreatedAt: null } }).score).toBe(0);
	});
	test('a ban elsewhere in the org is high on its own, without Steam', () => {
		const r = assessRisk({
			...base,
			steamEnabled: false,
			profile: null,
			bannedOn: [{ serverName: 'EU #2', reason: 'aimbot' }]
		});
		expect(r.level).toBe('high');
		expect(r.steamChecked).toBe(false);
		expect(r.reasons[0].text).toBe('Banned on EU #2: aimbot');
	});
	test('watchlist and lookalike names are medium together', () => {
		const r = assessRisk({
			...base,
			profile: clean,
			watched: { reason: 'griefing' },
			resembles: [{ name: 'T0XIC', steamId: '76561198000000001', serverName: 'EU #1' }]
		});
		expect(r.level).toBe('medium');
		expect(r.score).toBe(35);
	});
	test('the score is capped at 100', () => {
		const r = assessRisk({
			...base,
			profile: { ...clean, vacBans: 3, gameBans: 2, communityBanned: true, accountCreatedAt: now },
			bannedOn: [
				{ serverName: 'a', reason: '' },
				{ serverName: 'b', reason: '' }
			]
		});
		expect(r.score).toBe(100);
	});
});

describe('accountAgeDays', () => {
	test('whole days, never negative, null when unknown', () => {
		expect(accountAgeDays(new Date('2026-09-01T00:00:00Z'), now)).toBe(8);
		expect(accountAgeDays(new Date('2026-09-10T00:00:00Z'), now)).toBe(0);
		expect(accountAgeDays(null, now)).toBeNull();
	});
});

describe('names', () => {
	test('normaliseName undoes leetspeak and strips noise', () => {
		expect(normaliseName('T0X1C_AVENGER')).toBe('toxicavenger');
		expect(normaliseName('[AE] Sn4ke!')).toBe('aesnakei');
		expect(normaliseName('0xDEADBEEF')).toBe('oxdeadbeef');
	});
	test('levenshtein', () => {
		expect(levenshtein('kitten', 'sitting')).toBe(3);
		expect(levenshtein('', 'abc')).toBe(3);
		expect(levenshtein('same', 'same')).toBe(0);
	});
	test('namesResemble catches the obvious dodges and not short coincidences', () => {
		expect(namesResemble('T0XIC_AVENGER', 'ToxicAvenger')).toBe(true);
		expect(namesResemble('ToxicAvenger2', 'ToxicAvenger')).toBe(true);
		expect(namesResemble('Nomad', 'Nomad_')).toBe(true);
		expect(namesResemble('Brick', 'Brock')).toBe(false);
		expect(namesResemble('Bob', 'Rob')).toBe(false);
		expect(namesResemble('Late_to_the_party', 'Late to the party!!')).toBe(true);
		expect(namesResemble('Copperhead', 'Copperheed')).toBe(true);
		expect(namesResemble('Whistler', 'Wrangler')).toBe(false);
	});
});
