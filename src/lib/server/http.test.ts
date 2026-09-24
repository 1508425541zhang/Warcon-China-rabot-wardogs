import { describe, expect, test } from 'bun:test';
import { DrizzleQueryError } from 'drizzle-orm/errors';
import { addressKey, ApiError, CLIENT_IP_HEADER, forLog, publicMessage } from './http';

describe('publicMessage', () => {
	test('passes our own errors through', () => {
		expect(publicMessage(new ApiError(400, 'name is required.'))).toBe('name is required.');
	});

	test('hides everything else behind a generic line', () => {
		const quiet = console.error;
		console.error = () => {};
		try {
			expect(publicMessage(new Error('password authentication failed for user "warcon"'))).toBe(
				'Internal error.'
			);
			expect(publicMessage('boom', 'Poll failed.')).toBe('Poll failed.');
		} finally {
			console.error = quiet;
		}
	});
});

test('the stored lockout key is a keyed hash of the address, never the address', () => {
	const from = (ip: string) =>
		new Request('http://localhost/', { headers: ip ? { [CLIENT_IP_HEADER]: ip } : {} });
	const key = addressKey(from('203.0.113.7'), 'secret-one');
	expect(key).toMatch(/^[0-9a-f]{32}$/);
	expect(key).toBe(addressKey(from('203.0.113.7'), 'secret-one'));
	expect(key).not.toBe(addressKey(from('203.0.113.8'), 'secret-one'));
	expect(key).not.toBe(addressKey(from('203.0.113.7'), 'secret-two'));
	expect(addressKey(from(''), 'secret-one')).toBe('unknown');
});

test('a failed query is logged without its parameters: for the servers table they are the stored RCON password and the address', () => {
	const failed = new DrizzleQueryError(
		'insert into "servers" ("id", "host", "password_enc", "notes") values ($1, $2, $3, $4)',
		['s1', 'rcon.example.net', 'v1.aaaa.bbbbcccc', 'two\nlines'],
		new Error('duplicate key value violates unique constraint "servers_pkey"')
	);
	const logged = String(forLog(failed));
	expect(logged).toContain('insert into "servers"');
	expect(logged).toContain('duplicate key value');
	expect(logged).not.toContain('v1.aaaa.bbbbcccc');
	expect(logged).not.toContain('rcon.example.net');
	expect(logged).not.toContain('lines');
	expect(forLog('plain')).toBe('plain');
});
