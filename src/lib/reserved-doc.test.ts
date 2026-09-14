import { expect, test } from 'bun:test';
import { hasReservedKey, reservedFromText, reservedIntoText } from './reserved-doc';
import { parseIni, getScalar } from './config-doc';

// The TLR server's session section as GET /v1/config returns it on live build CL-501228.
const LIVE = [
	'[/Script/WDGame.WDGameSession]',
	'ServerName=[TLR][UK] The Last Rifles | Vanilla | tlr.gg #1',
	'ServerImageURL="https://i.ibb.co/x/logo.png"',
	'MaxReservedSlots=2',
	'!DefaultReservedPlayerIds=ClearArray',
	'.DefaultReservedPlayerIds=00000000000000000',
	'',
	'[MatchState.Playing.KOTH]',
	'ScorePeriod=24',
	''
].join('\r\n');

test('reads the live build form', () => {
	expect(reservedFromText(LIVE)).toEqual(['00000000000000000']);
	expect(hasReservedKey(LIVE)).toBe(true);
});

test('reads the quoted +Key form the reference ini uses', () => {
	const text =
		'[/Script/WDGame.WDGameSession]\n+DefaultReservedPlayerIds="76561198000000001"\n+DefaultReservedPlayerIds=76561198000000002\n';
	expect(reservedFromText(text)).toEqual(['76561198000000001', '76561198000000002']);
});

test('missing key or section is an empty list', () => {
	expect(reservedFromText('[/Script/WDGame.WDGameSession]\nServerName=x\n')).toEqual([]);
	expect(reservedFromText('[MatchState.Playing.KOTH]\nScorePeriod=24\n')).toEqual([]);
	expect(reservedFromText('')).toEqual([]);
	expect(hasReservedKey('[/Script/WDGame.WDGameSession]\nServerName=x\n')).toBe(false);
});

test('writes the list back in place, keeps CRLF and every other line', () => {
	const out = reservedIntoText(LIVE, ['00000000000000000', '76561198000000001']);
	expect(out).toContain('\r\n');
	expect(out.split('\r\n')).toEqual([
		'[/Script/WDGame.WDGameSession]',
		'ServerName=[TLR][UK] The Last Rifles | Vanilla | tlr.gg #1',
		'ServerImageURL="https://i.ibb.co/x/logo.png"',
		'MaxReservedSlots=2',
		'!DefaultReservedPlayerIds=ClearArray',
		'.DefaultReservedPlayerIds=00000000000000000',
		'.DefaultReservedPlayerIds=76561198000000001',
		'',
		'[MatchState.Playing.KOTH]',
		'ScorePeriod=24',
		''
	]);
	expect(reservedFromText(out)).toEqual(['00000000000000000', '76561198000000001']);
	expect(getScalar(parseIni(out), '/Script/WDGame.WDGameSession', 'MaxReservedSlots')).toBe('2');
});

test('removing the last id leaves an explicit empty array', () => {
	const out = reservedIntoText(LIVE, []);
	expect(out).toContain('!DefaultReservedPlayerIds=ClearArray');
	expect(out).not.toContain('.DefaultReservedPlayerIds=');
	expect(reservedFromText(out)).toEqual([]);
});

test('mixed +Key lines collapse into one clear-and-add block; a missing section is appended', () => {
	const mixed =
		'[/Script/WDGame.WDGameSession]\n+DefaultReservedPlayerIds="76561198000000001"\nServerName=x\n+DefaultReservedPlayerIds=76561198000000002\n';
	const out = reservedIntoText(mixed, ['76561198000000003']);
	expect(out).toBe(
		'[/Script/WDGame.WDGameSession]\n!DefaultReservedPlayerIds=ClearArray\n.DefaultReservedPlayerIds=76561198000000003\nServerName=x\n'
	);
	const appended = reservedIntoText('[MatchState.Playing.KOTH]\nScorePeriod=24\n', ['1']);
	expect(appended).toBe(
		'[MatchState.Playing.KOTH]\nScorePeriod=24\n\n[/Script/WDGame.WDGameSession]\n!DefaultReservedPlayerIds=ClearArray\n.DefaultReservedPlayerIds=1\n'
	);
});

test('a bare Key=value line is replaced too, so a removed id cannot come back', () => {
	const text =
		'[/Script/WDGame.WDGameSession]\nDefaultReservedPlayerIds=76561198000000001\n+DefaultReservedPlayerIds=76561198000000002\nServerName=x\n';
	expect(reservedFromText(text)).toEqual(['76561198000000001', '76561198000000002']);
	const out = reservedIntoText(text, ['76561198000000002']);
	expect(out).toBe(
		'[/Script/WDGame.WDGameSession]\n!DefaultReservedPlayerIds=ClearArray\n.DefaultReservedPlayerIds=76561198000000002\nServerName=x\n'
	);
	expect(reservedFromText(out)).toEqual(['76561198000000002']);
});
