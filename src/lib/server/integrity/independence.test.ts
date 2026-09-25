import { describe, expect, test } from 'bun:test';
import { independentEpisode } from './independence';

describe('independent punishment episodes', () => {
	const at = new Date('2026-01-01T00:00:00Z');
	const old = { eventIds: ['a'], roundId: 'round-1', clockTo: 180, observedAt: at };
	test('disjoint kills inside the same rolling episode cannot count twice', () => {
		expect(
			independentEpisode(
				old,
				{ eventIds: ['b'], roundId: 'round-1', clockFrom: 181, observedAt: at },
				180
			)
		).toBe(false);
		expect(
			independentEpisode(
				old,
				{ eventIds: ['b'], roundId: 'round-1', clockFrom: 360, observedAt: at },
				180
			)
		).toBe(true);
	});
	test('new round requires non-overlapping evidence; unknown round needs elapsed time', () => {
		expect(
			independentEpisode(
				old,
				{ eventIds: ['a'], roundId: 'round-2', clockFrom: 0, observedAt: at },
				180
			)
		).toBe(false);
		expect(
			independentEpisode(
				old,
				{ eventIds: ['b'], roundId: 'round-2', clockFrom: 0, observedAt: at },
				180
			)
		).toBe(true);
		expect(
			independentEpisode(
				{ ...old, roundId: null },
				{ eventIds: ['b'], roundId: 'round-2', clockFrom: 0, observedAt: at },
				180
			)
		).toBe(false);
	});
});
