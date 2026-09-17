import { describe, expect, test } from 'bun:test';
import {
	activeEntries,
	desiredOf,
	isAlreadyApplied,
	isGone,
	isUnreachable,
	planSync,
	type PlanInput,
	type StateLike
} from './lists-plan';

const now = new Date('2026-09-09T12:00:00Z');
const ago = (ms: number) => new Date(now.getTime() - ms);

describe('game error interpretation', () => {
	test('already applied', () => {
		expect(isAlreadyApplied({ status: 409, message: 'x' })).toBe(true);
		expect(isAlreadyApplied({ status: 400, message: 'Player is already banned.' })).toBe(true);
		expect(
			isAlreadyApplied({ status: 400, message: 'steamId must be a 17-digit SteamID64.' })
		).toBe(false);
	});
	test('a revision conflict is not "already applied"', () => {
		expect(isAlreadyApplied({ status: 412, code: 'revision_conflict', message: 'changed' })).toBe(
			false
		);
		expect(
			isAlreadyApplied({
				status: 409,
				code: 'already_reserved',
				message: 'SteamId 1 is already reserved.'
			})
		).toBe(true);
	});
	test('gone', () => {
		expect(isGone({ status: 404, message: 'x' })).toBe(true);
		expect(isGone({ status: 404, code: 'reserved_not_found', message: 'x' })).toBe(true);
		expect(isGone({ status: 400, code: 'not_found', message: 'x' })).toBe(true);
		expect(isGone({ status: 400, message: 'nope' })).toBe(false);
	});
});

test('activeEntries drops removed and expired rows', () => {
	const rows = [
		{ id: 'a', removedAt: null, expiresAt: null },
		{ id: 'b', removedAt: now, expiresAt: null },
		{ id: 'c', removedAt: null, expiresAt: ago(1) },
		{ id: 'd', removedAt: null, expiresAt: new Date(now.getTime() + 1) }
	];
	expect(activeEntries(rows, now).map((r) => r.id)).toEqual(['a', 'd']);
});

const state = (p: Partial<StateLike> & Pick<StateLike, 'kind' | 'steamId'>): StateLike => ({
	sourceListId: 'L',
	state: 'applied',
	error: '',
	attemptedAt: null,
	...p
});

const input = (p: Partial<PlanInput> = {}): PlanInput => ({
	now,
	retryAfterMs: 5 * 60_000,
	desired: { bans: [], reserved: [] },
	observed: { bans: [], reserved: [] },
	state: [],
	...p
});

const ban = (steamId: string, reason = '') => ({ steamId, reason, listId: 'L' });
const slot = (steamId: string, member = false) => ({ steamId, listId: 'L', member });

describe('planSync', () => {
	test('wanted and absent → add', () => {
		const p = planSync(input({ desired: { bans: [ban('1', 'cheat')], reserved: [] } }));
		expect(p.adds).toEqual([{ kind: 'ban', steamId: '1', listId: 'L', reason: 'cheat' }]);
		expect(p.removes).toEqual([]);
	});

	test('managed, present, no longer wanted → remove; managed and absent → drop the row', () => {
		const p = planSync(
			input({
				observed: { bans: ['1'], reserved: [] },
				state: [state({ kind: 'ban', steamId: '1' }), state({ kind: 'ban', steamId: '2' })]
			})
		);
		expect(p.removes).toEqual([{ kind: 'ban', steamId: '1' }]);
		expect(p.deletes).toEqual([{ kind: 'ban', steamId: '2' }]);
	});

	test('a coincidental local entry stays local: no add, no state row', () => {
		const p = planSync(
			input({
				desired: { bans: [ban('1')], reserved: [] },
				observed: { bans: ['1'], reserved: [] }
			})
		);
		expect(p.adds).toEqual([]);
		expect(p.confirms).toEqual([]);
		expect(p.local).toEqual([{ kind: 'ban', steamId: '1' }]);
	});

	test('managed and present but recorded as failed → confirm applied', () => {
		const p = planSync(
			input({
				desired: { bans: [ban('1')], reserved: [] },
				observed: { bans: ['1'], reserved: [] },
				state: [state({ kind: 'ban', steamId: '1', state: 'failed', error: 'x' })]
			})
		);
		expect(p.confirms.map((c) => c.steamId)).toEqual(['1']);
		expect(p.adds).toEqual([]);
	});

	test('managed but gone from the server → re-add (someone removed it by hand)', () => {
		const p = planSync(
			input({
				desired: { bans: [ban('1')], reserved: [] },
				state: [state({ kind: 'ban', steamId: '1' })]
			})
		);
		expect(p.adds.map((a) => a.steamId)).toEqual(['1']);
	});

	test('a failed add waits out the backoff, then retries', () => {
		const failed = state({
			kind: 'ban',
			steamId: '1',
			state: 'failed',
			error: 'Bad request',
			attemptedAt: ago(60_000)
		});
		expect(
			planSync(input({ desired: { bans: [ban('1')], reserved: [] }, state: [failed] })).adds
		).toEqual([]);
		expect(
			planSync(
				input({
					desired: { bans: [ban('1')], reserved: [] },
					state: [{ ...failed, attemptedAt: ago(10 * 60_000) }]
				})
			).adds.map((a) => a.steamId)
		).toEqual(['1']);
	});

	test('every wanted reserved slot is added: the list has no cap, members included', () => {
		const p = planSync(
			input({
				desired: {
					bans: [],
					reserved: [slot('a'), slot('b'), slot('c'), slot('member', true)]
				},
				observed: { bans: [], reserved: ['local', 'b'] }
			})
		);
		expect(p.adds.map((a) => a.steamId)).toEqual(['a', 'c', 'member']);
		expect(p.local).toEqual([{ kind: 'reserve', steamId: 'b' }]);
	});

	test('a managed reserved slot no longer wanted is removed; a local one is left alone', () => {
		const p = planSync(
			input({
				desired: { bans: [], reserved: [slot('new')] },
				observed: { bans: [], reserved: ['local', 'stale'] },
				state: [state({ kind: 'reserve', steamId: 'stale' })]
			})
		);
		expect(p.removes).toEqual([{ kind: 'reserve', steamId: 'stale' }]);
		expect(p.adds.map((a) => a.steamId)).toEqual(['new']);
	});
});

test('isGone: a missing entry is gone, a missing route is not', () => {
	expect(isGone({ status: 404, code: 'ban_not_found', message: 'not banned' })).toBe(true);
	expect(isGone({ status: 404, code: 'not_found', message: 'gone' })).toBe(true);
	expect(isGone({ status: 404, code: 'no_route', message: 'not served' })).toBe(false);
});

test('isUnreachable: outages and rate limiting both stop the run; ordinary refusals do not', () => {
	expect(isUnreachable({ status: 502, code: 'unreachable', message: 'x' })).toBe(true);
	expect(isUnreachable({ status: 429, code: 'rate_limited', message: 'slow down' })).toBe(true);
	expect(isUnreachable({ status: 409, code: 'already_reserved', message: 'already' })).toBe(false);
	expect(isUnreachable({ status: 400, message: 'bad id' })).toBe(false);
});

describe('desiredOf', () => {
	test("a player on the org list and the server's own list is wanted once, from the org list", () => {
		const want = desiredOf([
			{ kind: 'reserve', steamId: '1', reason: 'donor here', listId: 'srv', serverId: 's1' },
			{ kind: 'reserve', steamId: '1', reason: 'donor', listId: 'org', serverId: null },
			{ kind: 'reserve', steamId: '2', reason: '', listId: 'srv', serverId: 's1' },
			{ kind: 'ban', steamId: '3', reason: 'cheating', listId: 'bans', serverId: null }
		]);
		expect(want.reserved).toEqual([
			{ steamId: '1', listId: 'org', member: false },
			{ steamId: '2', listId: 'srv', member: false }
		]);
		expect(want.bans).toEqual([{ steamId: '3', reason: 'cheating', listId: 'bans' }]);
	});
});
