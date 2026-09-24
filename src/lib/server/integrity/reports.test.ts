import { describe, expect, test } from 'bun:test';
import { parseReportCommand, resolveReportTarget } from './reports';

const roster = [
	{ steamId: '76561198000000001', name: 'Alice' },
	{ steamId: '76561198000000002', name: 'AliceTwo' },
	{ steamId: '76561198000000003', name: 'Bob' }
];

describe('community report input', () => {
	test('chat command parser preserves reason and never creates a ban action', () => {
		expect(parseReportCommand('!report "Alice Two" suspected wallhack')).toEqual({
			target: 'Alice Two',
			reason: 'suspected wallhack'
		});
		expect(parseReportCommand('!BAN 76561198000000001 repeated teamkills')).toEqual({
			target: '76561198000000001',
			reason: 'repeated teamkills'
		});
		expect(parseReportCommand('!ban Alice')).toBeNull();
	});

	test('identity resolution prefers exact names and rejects fuzzy ambiguity', () => {
		expect(resolveReportTarget('Alice', roster).steamId).toBe('76561198000000001');
		expect(resolveReportTarget('76561198000000003', roster).name).toBe('Bob');
		expect(() => resolveReportTarget('ali', roster)).toThrow();
		expect(() => resolveReportTarget('no such player', roster)).toThrow();
	});
});
