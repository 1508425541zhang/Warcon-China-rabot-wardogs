import { test, expect } from 'bun:test';
import {
	shortRiskDecision,
	freshShortSnapshot,
	SHORT_WARNING,
	SHORT_KICK,
	SHORT_RISK_SHA
} from './short-risk-policy';
import { shortWindowConsensus } from './short-risk-policy';
test('five distinct consecutive P99.9 windows require recency-weighted consensus', () => {
	const end = Date.now();
	const windows = Array.from({ length: 5 }, (_, i) => ({
		at: new Date(end - (4 - i) * 10000).toISOString(),
		score: SHORT_KICK + 0.001 * (i + 1)
	}));
	expect(shortWindowConsensus(windows, windows[4].at)).toBeCloseTo(
		SHORT_KICK + (0.001 * 55) / 15,
		12
	);
	expect(shortWindowConsensus(windows.slice(1), windows[4].at)).toBeNull();
	expect(
		shortWindowConsensus(
			windows.map((x, i) => ({ ...x, score: i === 1 ? SHORT_KICK : x.score })),
			windows[4].at
		)
	).toBeNull();
	expect(
		shortWindowConsensus(
			windows.map((x) => ({ ...x, at: windows[4].at })),
			windows[4].at
		)
	).toBeNull();
	expect(
		shortWindowConsensus(
			windows.map((x, i) => ({ ...x, at: i === 0 ? new Date(end - 100000).toISOString() : x.at })),
			windows[4].at
		)
	).toBeNull();
	expect(shortWindowConsensus(windows, new Date(end + 1000).toISOString())).toBeNull();
});
test('warning does not kick; strict upper-tail ties are not escalated', () => {
	expect(shortRiskDecision(SHORT_WARNING)).toBe('normal');
	expect(shortRiskDecision(SHORT_WARNING + 1e-8)).toBe('warning');
	expect(shortRiskDecision(SHORT_KICK)).toBe('warning');
	expect(shortRiskDecision(SHORT_KICK + 1e-8)).toBe('kick');
	expect(shortRiskDecision(NaN)).toBe('unavailable');
});
test('stale, future, old classifier and mismatched model results cannot drive actions', () => {
	const now = Date.now();
	const x = {
		evaluated_at: new Date(now).toISOString(),
		model_sha256: SHORT_RISK_SHA,
		algorithm: 'IsolationForest',
		players: []
	};
	expect(freshShortSnapshot(x, now)).toBe(true);
	expect(freshShortSnapshot(x, now + 30001)).toBe(false);
	expect(freshShortSnapshot(x, now - 1)).toBe(false);
	expect(freshShortSnapshot({ ...x, model_sha256: 'old-classifier' }, now)).toBe(false);
	expect(freshShortSnapshot({ ...x, algorithm: 'XGBoost' }, now)).toBe(false);
});
