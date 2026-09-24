import { describe, expect, test } from 'bun:test';
import { assessEnrolment, enrolmentStatus, type AuthMethods } from './enrolment';

const base: AuthMethods = {
	password: false,
	twoFactor: false,
	passkeys: 0,
	providers: [],
	recoveryKey: false
};

describe('assessEnrolment', () => {
	test('a bare password account fails on both counts', () => {
		const e = assessEnrolment({ ...base, password: true, providers: ['credential'] }, 'member');
		expect(e.complete).toBe(false);
		expect(e.waysIn).toBe(0);
		expect(e.problems).toHaveLength(2);
	});
	test('password + authenticator is one way in, still short of two', () => {
		const e = assessEnrolment(
			{ ...base, password: true, twoFactor: true, providers: ['credential'] },
			'member'
		);
		expect(e.waysIn).toBe(1);
		expect(e.complete).toBe(false);
		expect(e.problems[0]).toMatch(/only one way in/);
	});
	test('passkey + Discord satisfies a member and an owner', () => {
		const m = { ...base, passkeys: 1, providers: ['discord'] };
		expect(assessEnrolment(m, 'member').complete).toBe(true);
		expect(assessEnrolment(m, 'owner').complete).toBe(true);
	});
	test('two passkeys are enough for a member but an owner needs SSO or a recovery key', () => {
		const m = { ...base, passkeys: 2 };
		expect(assessEnrolment(m, 'member').complete).toBe(true);
		const o = assessEnrolment(m, 'owner');
		expect(o.complete).toBe(false);
		expect(o.problems[0]).toMatch(/Owners/);
		expect(assessEnrolment({ ...m, recoveryKey: true }, 'owner').complete).toBe(true);
	});
	test('password + 2FA + recovery key is complete for an owner', () => {
		const m = {
			...base,
			password: true,
			twoFactor: true,
			providers: ['credential'],
			recoveryKey: true
		};
		expect(assessEnrolment(m, 'owner')).toEqual({ waysIn: 2, complete: true, problems: [] });
	});
	test('a self-declared steam id is not a provider; only a linked account counts', () => {
		expect(
			assessEnrolment({ ...base, passkeys: 1, providers: ['credential'] }, 'member').waysIn
		).toBe(1);
		expect(assessEnrolment({ ...base, passkeys: 1, providers: ['steam'] }, 'member').waysIn).toBe(
			2
		);
	});
});

describe('enrolmentStatus', () => {
	const s = { authGraceDays: 14, authMemberGraceDays: 30 };
	const day = 86_400_000;
	test('complete accounts are never due', () => {
		expect(
			enrolmentStatus(
				{ role: 'owner', authComplete: true, authGraceStartedAt: new Date(0).toISOString() },
				s
			).due
		).toBe(false);
	});
	test('grace has not started until the first sign-in', () => {
		const st = enrolmentStatus({ role: 'owner', authComplete: false, authGraceStartedAt: null }, s);
		expect(st).toEqual({ due: false, daysLeft: null, deadline: null });
	});
	test('owners get the shorter grace', () => {
		const started = new Date(1_000_000 * day).toISOString();
		const now = 1_000_000 * day + 13 * day;
		expect(
			enrolmentStatus({ role: 'owner', authComplete: false, authGraceStartedAt: started }, s, now)
		).toMatchObject({ due: false, daysLeft: 1 });
		expect(
			enrolmentStatus(
				{ role: 'owner', authComplete: false, authGraceStartedAt: started },
				s,
				now + day
			).due
		).toBe(true);
		expect(
			enrolmentStatus(
				{ role: 'member', authComplete: false, authGraceStartedAt: started },
				s,
				now + day
			)
		).toMatchObject({ due: false, daysLeft: 16 });
	});
	test('zero days enforces at once', () => {
		const started = new Date().toISOString();
		expect(
			enrolmentStatus(
				{ role: 'owner', authComplete: false, authGraceStartedAt: started },
				{ authGraceDays: 0, authMemberGraceDays: 0 },
				Date.now() + 1
			).due
		).toBe(true);
	});
});
