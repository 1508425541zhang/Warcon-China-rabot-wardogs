import { describe, expect, test } from 'bun:test';
import { appliesFor, FIELDS, lockedFor, lockedKeys, S_SESSION } from './config-fields';
import type { ConfigSection } from './types';

const field = (key: string) => FIELDS.find((f) => f.key === key)!;

// The schema the TLR server reports on live build CL-501228 (2026-09-14), trimmed to what matters.
const SECTIONS: ConfigSection[] = [
	{
		section: S_SESSION,
		appliesWhen: 'applied',
		description: 'Server identity.',
		allowedKeys: ['ServerName', 'ServerPassword', 'ServerImageURL'],
		keyOverrides: [
			{
				key: 'ServerPassword',
				appliesWhen: 'applied',
				description: 'Gates new joins at once.',
				writable: true
			},
			{
				key: 'ServerImageURL',
				appliesWhen: 'pending',
				description: 'Fetched off-thread.',
				writable: true
			},
			{
				key: 'ServerName',
				appliesWhen: 'applied',
				description:
					"Pinned by -RCON_FixedServerName on this server's command line. The value is shown but cannot be changed here.",
				writable: false,
				lockedBy: 'RCON_FixedServerName'
			}
		]
	},
	{
		section: '/Script/WDRCON.WDRCONSettings',
		appliesWhen: 'next-restart',
		description: 'RCON listener.',
		allowedKeys: [
			'bEnabled',
			'BindAddress',
			'bWriteAuditLogFile',
			'Password',
			'PasswordHash',
			'Port'
		],
		keyOverrides: [
			{
				key: 'Port',
				appliesWhen: 'next-restart',
				description: 'Pinned by -RCONPort.',
				writable: false,
				lockedBy: 'RCONPort'
			}
		]
	},
	{
		section: 'MatchState.Playing.KOTH',
		appliesWhen: 'next-match',
		description: 'Scoring.',
		allowedKeys: ['ScorePeriod'],
		keyOverrides: []
	},
	// CL-499480's document had no keyOverrides on some sections: the console's fallback applies.
	{ section: '/Script/Engine.GameSession', appliesWhen: 'next-restart', description: 'Slots.' }
];

describe('appliesFor', () => {
	test('a key override wins over its section', () => {
		expect(appliesFor(field('imageUrl'), SECTIONS)).toEqual({
			state: 'pending',
			description: 'Fetched off-thread.'
		});
		expect(appliesFor(field('serverName'), SECTIONS)?.state).toBe('applied');
	});
	test('an empty override list answers for all keys; no list or no section falls back', () => {
		expect(appliesFor(field('scorePeriod'), SECTIONS)).toEqual({
			state: 'next-match',
			description: 'Scoring.'
		});
		expect(appliesFor(field('maxPlayers'), SECTIONS)).toEqual(field('maxPlayers').appliesFallback!);
		expect(appliesFor(field('maxPlayers'), [])).toEqual(field('maxPlayers').appliesFallback!);
	});
});

describe('lockedFor', () => {
	test('only writable: false pins a field, case-insensitively, carrying the switch', () => {
		expect(lockedFor(field('serverName'), SECTIONS)).toEqual({
			section: S_SESSION,
			key: 'ServerName',
			lockedBy: 'RCON_FixedServerName',
			description: expect.stringContaining('RCON_FixedServerName')
		});
		expect(lockedFor(field('serverPassword'), SECTIONS)).toBeNull();
		expect(lockedFor(field('imageUrl'), SECTIONS)).toBeNull();
		expect(lockedFor(field('scorePeriod'), SECTIONS)).toBeNull();
		expect(lockedFor(field('serverName'), [])).toBeNull();
	});
	test('an override without writable (CL-499480 schema) is editable', () => {
		const old: ConfigSection[] = [
			{
				section: S_SESSION,
				appliesWhen: 'applied',
				keyOverrides: [{ key: 'ServerName', appliesWhen: 'applied', description: 'x' }]
			}
		];
		expect(lockedFor(field('serverName'), old)).toBeNull();
	});
});

test('lockedKeys lists every pinned key, including ones the form has no field for', () => {
	expect(lockedKeys(SECTIONS).map((k) => `${k.key}:${k.lockedBy}`)).toEqual([
		'ServerName:RCON_FixedServerName',
		'Port:RCONPort'
	]);
	expect(lockedKeys([])).toEqual([]);
});
