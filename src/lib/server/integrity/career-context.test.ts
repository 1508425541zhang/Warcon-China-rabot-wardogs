import { describe, expect, test } from 'bun:test';
import { summarizeCleanCareer } from './career-context';

describe('clean personal career reference', () => {
	test('deduplicates overlapping kill snapshots and preserves long-term quantiles', () => {
		const start = Date.parse('2026-01-01T00:00:00Z');
		const rows = Array.from({ length: 120 }, (_, i) => ({
			roundId: `round-${Math.floor(i / 10)}`,
			observedAt: new Date(start + Math.floor(i / 10) * 86_400_000 + (i % 10) * 180_000),
			kpm180: i < 117 ? 1 + (i % 3) * 0.1 : 5,
			headshotRate: 0.2,
			maxKills15s: i < 117 ? 2 : 8
		}));
		const withOverlaps = rows.flatMap((row) => [
			row,
			{ ...row, observedAt: new Date(row.observedAt.getTime() + 1_000), kpm180: 99 }
		]);
		const summary = summarizeCleanCareer(withOverlaps)!;
		expect(summary.sampleCount).toBe(120);
		expect(summary.uniqueDays).toBe(12);
		expect(summary.lifetimeMatches).toBe(12);
		expect(summary.kpmMedian).toBe(1.1);
		expect(summary.kpmDistribution.p99).toBe(5);
		expect(summary.orderedKpm.at(-1)).toBe(5);
		expect(summary.recent7d?.sampleCount).toBeLessThan(120);
	});
});
