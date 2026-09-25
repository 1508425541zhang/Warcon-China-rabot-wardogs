import type { Status } from '$lib/types';

const record = (value: unknown): Record<string, unknown> | null =>
	value !== null && typeof value === 'object' && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: null;
const finite = (value: unknown): number | null => {
	if (typeof value !== 'number' && !(typeof value === 'string' && /^-?\d+(?:\.\d+)?$/.test(value)))
		return null;
	const n = Number(value);
	return Number.isFinite(n) ? n : null;
};

export function normalizeScores(value: unknown): { name: string; score: number }[] | null {
	if (!Array.isArray(value)) return null;
	const scores = value.flatMap((entry) => {
		const item = record(entry);
		const score = finite(item?.score);
		return item && typeof item.name === 'string' && item.name && score !== null
			? [{ name: item.name, score }]
			: [];
	});
	return scores.length ? scores : null;
}

export function normalizeStatus(value: unknown): Status | null {
	const item = record(value);
	if (!item || typeof item.serverName !== 'string' || typeof item.map !== 'string') return null;
	return {
		serverName: item.serverName,
		map: item.map,
		experiences: Array.isArray(item.experiences)
			? item.experiences.filter((v): v is string => typeof v === 'string')
			: [],
		lighting: typeof item.lighting === 'string' ? item.lighting : '',
		alternator: typeof item.alternator === 'string' ? item.alternator : '',
		scoreTick: finite(item.scoreTick),
		scoreTickMin: finite(item.scoreTickMin),
		scoreTickMax: finite(item.scoreTickMax),
		scoreCap: finite(item.scoreCap),
		matchSeconds: finite(item.matchSeconds),
		playerCount: Math.max(0, finite(item.playerCount) ?? 0),
		maxPlayers: Math.max(0, finite(item.maxPlayers) ?? 0),
		scores: (normalizeScores(item.scores) ?? []).map((score) => {
			const raw = Array.isArray(item.scores)
				? item.scores.find((entry) => record(entry)?.name === score.name)
				: null;
			const color = record(raw)?.colorHex;
			return {
				...score,
				colorHex: typeof color === 'string' && /^#[0-9a-fA-F]{6}$/.test(color) ? color : '#888888'
			};
		}),
		rotationNow: finite(item.rotationNow) ?? 0,
		rotationNext: finite(item.rotationNext) ?? 0
	};
}

export function normalizeWebhookEvents(value: unknown): string[] {
	return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : [];
}
/** Null is an explicit all-server scope; malformed JSON is never widened to all servers. */
export function webhookScopeAllows(value: unknown, serverId: string): boolean {
	return (
		value === null ||
		(Array.isArray(value) &&
			value.every((v) => typeof v === 'string') &&
			(value.length === 0 || value.includes(serverId)))
	);
}
