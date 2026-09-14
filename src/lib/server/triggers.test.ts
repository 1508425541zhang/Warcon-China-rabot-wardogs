import { describe, expect, test } from 'bun:test';
import {
	factionChangeTargets,
	onTarget,
	renderTemplate,
	restartNoticeStage,
	riskKickVerdict,
	validateConfig,
	welcomeTargets
} from './trigger-rules';
import type { RiskKickConfig } from './trigger-rules';

describe('validateConfig', () => {
	test('welcome needs a message and trims it to 200 characters', () => {
		expect(() => validateConfig('welcome', { message: '  ' })).toThrow('empty');
		const c = validateConfig('welcome', { message: 'x'.repeat(300), onlyFirstVisit: 'yes' });
		expect(c).toEqual({ message: 'x'.repeat(200), onlyFirstVisit: true, afterFaction: false });
		expect(validateConfig('welcome', { message: 'hi', afterFaction: 1 })).toMatchObject({
			afterFaction: true
		});
	});
	test('faction_change needs a message', () => {
		expect(() => validateConfig('faction_change', {})).toThrow('empty');
		expect(validateConfig('faction_change', { message: ' hi ' })).toEqual({ message: 'hi' });
	});
	test('broadcast accepts a newline-separated string and drops blanks', () => {
		const c = validateConfig('broadcast', {
			messages: 'a\n\n b \n',
			everyMinutes: '15',
			minPlayers: -3
		});
		expect(c).toEqual({ messages: ['a', 'b'], everyMinutes: 15, minPlayers: 0 });
		expect(() => validateConfig('broadcast', { messages: [], everyMinutes: 5 })).toThrow(
			'at least one'
		);
		expect(() => validateConfig('broadcast', { messages: ['a'], everyMinutes: 0 })).toThrow(
			'1-1440'
		);
	});
	test('empty_reset needs a map and a wait', () => {
		expect(() => validateConfig('empty_reset', { afterMinutes: 5 })).toThrow('map');
		const c = validateConfig('empty_reset', {
			map: 'Kavkazi',
			experiences: ['Bakurani_KOTH_01'],
			afterMinutes: 20
		});
		expect(c).toMatchObject({
			map: 'Kavkazi',
			afterMinutes: 20,
			cooldownMinutes: 30,
			lighting: ''
		});
	});
	test('restart_notice needs the main message, and a heads-up message when the heads-up is on', () => {
		expect(() => validateConfig('restart_notice', {})).toThrow('empty');
		expect(() =>
			validateConfig('restart_notice', { message: 'bye', leadMinutes: 30, leadMessage: '' })
		).toThrow('heads-up');
		expect(validateConfig('restart_notice', { message: ' bye ', leadMinutes: 0 })).toEqual({
			message: 'bye',
			leadMinutes: 0,
			leadMessage: '',
			repeatMinutes: 0,
			minPlayers: 1
		});
		// the heads-up cannot be earlier than the game start
		expect(
			validateConfig('restart_notice', { message: 'bye', leadMinutes: 5000, leadMessage: 'soon' })
		).toMatchObject({ leadMinutes: 719 });
	});
	test('risk_kick refuses an empty rule set and defaults the reason', () => {
		expect(() => validateConfig('risk_kick', {})).toThrow('at least one rule');
		const c = validateConfig('risk_kick', { vacBans: true }) as RiskKickConfig;
		expect(c.spareReserved).toBe(true);
		expect(c.reason).toContain('requirements');
	});
});

describe('welcomeTargets', () => {
	const a = { steamId: 'a' };
	const b = { steamId: 'b' };
	const c = { steamId: 'c' };
	const tick = {
		joined: [a],
		factioned: [
			{ player: b, from: null },
			{ player: c, from: 'Valkyra' }
		],
		firstVisit: new Set(['a', 'c'])
	};
	test('whispers joiners by default, and only first faction picks when waiting for one', () => {
		expect(welcomeTargets({ onlyFirstVisit: false, afterFaction: false }, tick)).toEqual([a]);
		expect(welcomeTargets({ onlyFirstVisit: false, afterFaction: true }, tick)).toEqual([b]);
	});
	test('first-visit applies to whichever list is in use', () => {
		expect(welcomeTargets({ onlyFirstVisit: true, afterFaction: false }, tick)).toEqual([a]);
		expect(welcomeTargets({ onlyFirstVisit: true, afterFaction: true }, tick)).toEqual([]);
	});
	test('faction changes are switches only, never the first pick', () => {
		expect(factionChangeTargets(tick).map((f) => f.player)).toEqual([c]);
	});
});

describe('renderTemplate', () => {
	test('fills placeholders case-insensitively and leaves unknown ones', () => {
		expect(
			renderTemplate('Hi {NAME}, welcome to {server} ({players}/{max}) on {map} {nope}', {
				name: 'Nomad',
				server: 'EU #1',
				players: 3,
				max: 64,
				map: 'Kavkazi'
			})
		).toBe('Hi Nomad, welcome to EU #1 (3/64) on Kavkazi {nope}');
	});
	test('clips to 200 characters', () => {
		expect(renderTemplate('{name}', { name: 'y'.repeat(500) })).toHaveLength(200);
	});
});

describe('riskKickVerdict', () => {
	const cfg: RiskKickConfig = {
		vacBans: true,
		gameBans: false,
		minAccountDays: 30,
		privateProfiles: false,
		bannedElsewhere: true,
		watchlist: true,
		spareReserved: true,
		reason: 'no'
	};
	const now = new Date('2026-09-09T12:00:00Z');
	const profile = {
		steamId: '76561198000000001',
		persona: '',
		avatar: '',
		profileUrl: '',
		public: true,
		accountCreatedAt: new Date('2026-09-01T00:00:00Z'),
		vacBans: 0,
		gameBans: 0,
		daysSinceLastBan: null,
		communityBanned: false,
		economyBan: 'none',
		fetchedAt: now,
		error: ''
	};
	const base = { profile, steamEnabled: true, bannedOn: [], watched: null, reserved: false, now };

	test('reserved slots are spared', () => {
		expect(
			riskKickVerdict(cfg, { ...base, reserved: true, bannedOn: [{ serverName: 'x', reason: '' }] })
		).toBeNull();
		expect(
			riskKickVerdict(
				{ ...cfg, spareReserved: false },
				{ ...base, reserved: true, bannedOn: [{ serverName: 'x', reason: '' }] }
			)
		).toBe('banned on x');
	});
	test('order: ban elsewhere, watchlist, VAC, age', () => {
		expect(riskKickVerdict(cfg, { ...base, watched: { reason: 'tk' } })).toBe(
			'on the watchlist (tk)'
		);
		expect(riskKickVerdict(cfg, { ...base, profile: { ...profile, vacBans: 2 } })).toBe(
			'2 VAC bans on record'
		);
		expect(riskKickVerdict(cfg, base)).toBe('Steam account only 8 days old (minimum 30)');
		expect(
			riskKickVerdict(cfg, {
				...base,
				profile: { ...profile, accountCreatedAt: new Date('2020-01-01') }
			})
		).toBeNull();
	});
	test('private profiles pass unless asked to fail', () => {
		const priv = { ...profile, public: false, accountCreatedAt: null };
		expect(riskKickVerdict(cfg, { ...base, profile: priv })).toBeNull();
		expect(riskKickVerdict({ ...cfg, privateProfiles: true }, { ...base, profile: priv })).toBe(
			'private profile, account age unknown'
		);
	});
	test('without Steam only the local rules apply', () => {
		expect(riskKickVerdict(cfg, { ...base, steamEnabled: false, profile: null })).toBeNull();
		expect(
			riskKickVerdict(cfg, { ...base, profile: { ...profile, error: 'Not found on Steam.' } })
		).toBeNull();
	});
});

describe('onTarget', () => {
	test('map alone when the rule names no experiences, else the set must match', () => {
		expect(
			onTarget({ map: 'Kavkazi', experiences: [] }, { map: 'Kavkazi', experiences: ['a'] })
		).toBe(true);
		expect(
			onTarget(
				{ map: 'Kavkazi', experiences: ['b', 'a'] },
				{ map: 'Kavkazi', experiences: ['a', 'b'] }
			)
		).toBe(true);
		expect(
			onTarget({ map: 'Kavkazi', experiences: ['a'] }, { map: 'Kavkazi', experiences: ['a', 'b'] })
		).toBe(false);
		expect(onTarget({ map: 'Europe', experiences: [] }, { map: 'Kavkazi', experiences: [] })).toBe(
			false
		);
	});
});

describe('restartNoticeStage', () => {
	const H = 3600_000;
	const start = Date.parse('2026-09-14T00:00:00Z');
	const cfg = { leadMinutes: 30, repeatMinutes: 0, minPlayers: 1 };
	const at = (hours: number, players = 10) => ({
		startedAt: start,
		playerCount: players,
		now: start + hours * H
	});

	test('nothing before the heads-up, nothing without a start time or players', () => {
		expect(restartNoticeStage(cfg, null, at(9))).toBeNull();
		expect(restartNoticeStage(cfg, null, { ...at(11.9), startedAt: 0 })).toBeNull();
		expect(restartNoticeStage(cfg, null, at(11.9, 0))).toBeNull();
	});
	test('the heads-up goes once inside the lead window, then the main message once due', () => {
		const lead = restartNoticeStage(cfg, null, at(11.6))!;
		expect(lead.stage).toBe('lead');
		expect(lead.minutes).toBe(24);
		expect(lead.state).toEqual({ startedAt: start, leadAt: start + 11.6 * H });
		expect(restartNoticeStage(cfg, lead.state, at(11.8))).toBeNull();
		const due = restartNoticeStage(cfg, lead.state, at(12.1))!;
		expect(due.stage).toBe('due');
		expect(due.minutes).toBe(0);
		expect(due.state.dueAt).toBe(start + 12.1 * H);
		expect(restartNoticeStage(cfg, due.state, at(13))).toBeNull();
	});
	test('a missed heads-up is skipped, not sent late, once the window is open', () => {
		const hit = restartNoticeStage(cfg, null, at(12.5))!;
		expect(hit.stage).toBe('due');
		expect(hit.state.leadAt).toBeUndefined();
	});
	test('repeat resends the main message on its cadence while the window stays open', () => {
		const c = { ...cfg, repeatMinutes: 15 };
		const first = restartNoticeStage(c, null, at(12))!;
		expect(restartNoticeStage(c, first.state, at(12.2))).toBeNull();
		const again = restartNoticeStage(c, first.state, at(12.3))!;
		expect(again.stage).toBe('due');
		expect(again.state.dueAt).toBe(start + 12.3 * H);
	});
	test('a new game start resets the cycle', () => {
		const old = { startedAt: start - 20 * H, leadAt: 1, dueAt: 2 };
		expect(restartNoticeStage(cfg, old, at(11.7))!.stage).toBe('lead');
	});
	test('no heads-up when leadMinutes is 0', () => {
		expect(restartNoticeStage({ ...cfg, leadMinutes: 0 }, null, at(11.9))).toBeNull();
		expect(restartNoticeStage({ ...cfg, leadMinutes: 0 }, null, at(12))!.stage).toBe('due');
	});
});
