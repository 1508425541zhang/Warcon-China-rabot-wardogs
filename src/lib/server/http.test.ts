import { describe, expect, test } from 'bun:test';
import { addressKey, ApiError, CLIENT_IP_HEADER, publicMessage } from './http';

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
