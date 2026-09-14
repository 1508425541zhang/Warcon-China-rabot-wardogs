import { describe, expect, test } from 'bun:test';
import { seenFilters } from './seen';

const parse = (q: string) => seenFilters(new URLSearchParams(q));

describe('seenFilters', () => {
	test('defaults: everyone, newest last seen first', () => {
		expect(parse('')).toEqual({
			q: '',
			since: null,
			serverId: '',
			flag: '',
			sort: 'lastSeen',
			dir: 'desc'
		});
	});

	test('name sort defaults to ascending, others to descending', () => {
		expect(parse('sort=name')).toMatchObject({ sort: 'name', dir: 'asc' });
		expect(parse('sort=minutes')).toMatchObject({ sort: 'minutes', dir: 'desc' });
		expect(parse('sort=minutes&dir=asc')).toMatchObject({ sort: 'minutes', dir: 'asc' });
	});

	test('unknown sort, flag and since fall back', () => {
		expect(parse('sort=drop%20table&flag=x&since=abc')).toMatchObject({
			sort: 'lastSeen',
			flag: '',
			since: null
		});
		expect(parse('since=0')).toMatchObject({ since: null });
		expect(parse('since=99999')).toMatchObject({ since: 3650 });
	});

	test('search is trimmed and capped', () => {
		expect(parse('q=%20%20bob%20%20')).toMatchObject({ q: 'bob' });
		expect(parse(`q=${'a'.repeat(200)}`).q).toHaveLength(100);
	});
});
