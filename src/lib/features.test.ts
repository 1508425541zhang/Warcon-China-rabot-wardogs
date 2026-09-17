import { describe, expect, test } from 'bun:test';
import {
	effectiveFeatures,
	featureOn,
	featureState,
	NO_ALLOWANCES,
	NO_SWITCHES,
	NOT_ALLOWED
} from './features';

const allowAll = { allowPublicStatus: true, allowPublicLeaderboards: true };
const wantAll = { publicStatus: true, publicLeaderboards: true };

describe('effectiveFeatures', () => {
	test('nothing is public by default, whatever the server asks for', () => {
		expect(effectiveFeatures(NO_ALLOWANCES, NO_SWITCHES)).toEqual({
			status: false,
			leaderboards: false
		});
		expect(effectiveFeatures(NO_ALLOWANCES, wantAll)).toEqual({
			status: false,
			leaderboards: false
		});
	});
	test('an allowance alone opens nothing; the server switch is still needed', () => {
		expect(effectiveFeatures(allowAll, NO_SWITCHES)).toEqual({
			status: false,
			leaderboards: false
		});
		expect(effectiveFeatures(allowAll, wantAll)).toEqual({ status: true, leaderboards: true });
	});
	test('each feature is judged on its own pair of switches', () => {
		const org = { allowPublicStatus: true, allowPublicLeaderboards: false };
		expect(effectiveFeatures(org, wantAll)).toEqual({ status: true, leaderboards: false });
		expect(featureOn(org, { publicStatus: false, publicLeaderboards: true }, 'status')).toBe(false);
	});
});

describe('featureState', () => {
	test('a disallowed switch carries the reason, an allowed one none', () => {
		expect(featureState(NO_ALLOWANCES, wantAll, 'leaderboards')).toEqual({
			on: false,
			allowed: false,
			wanted: true,
			reason: NOT_ALLOWED
		});
		expect(featureState(allowAll, NO_SWITCHES, 'status')).toEqual({
			on: false,
			allowed: true,
			wanted: false,
			reason: null
		});
	});
});
