import { z } from 'zod';
import type { Player } from './types';

export const groupConfigSchema = z
	.object({
		mode: z.enum(['off', 'manual', 'auto']),
		engine: z.enum(['structured', 'ai']),
		prefixLength: z.number().int().min(2).max(16),
		similarityPercent: z.number().min(0).max(99),
		minPlayers: z.number().int().min(2).max(100)
	})
	.strict();
export type GroupConfig = z.infer<typeof groupConfigSchema>;
export const defaultGroupConfig: GroupConfig = {
	mode: 'off',
	engine: 'structured',
	prefixLength: 4,
	similarityPercent: 70,
	minPlayers: 4
};
export function namePrefix(name: string, length: number): string {
	return Array.from(
		name
			.normalize('NFKC')
			.toLowerCase()
			.replace(/[^\p{L}\p{N}]/gu, '')
	)
		.slice(0, length)
		.join('');
}
/** Normalized Levenshtein similarity; punctuation, case and compatibility forms are ignored. */
export function prefixSimilarity(a: string, b: string): number {
	const x = Array.from(a),
		y = Array.from(b);
	if (!x.length || !y.length) return 0;
	let row = Array.from({ length: y.length + 1 }, (_, i) => i);
	for (let i = 1; i <= x.length; i++) {
		const next = [i];
		for (let j = 1; j <= y.length; j++)
			next[j] = Math.min(next[j - 1] + 1, row[j] + 1, row[j - 1] + (x[i - 1] === y[j - 1] ? 0 : 1));
		row = next;
	}
	return 100 * (1 - row[y.length] / Math.max(x.length, y.length));
}
export type GroupMember = Pick<Player, 'steamId' | 'name'> & { prefix: string };
export type SuspectedGroup = {
	id: string;
	faction: string;
	minimumSimilarity: number;
	members: GroupMember[];
};
export function detectGroups(
	players: readonly Player[],
	config: GroupConfig,
	factions: readonly string[]
): SuspectedGroup[] {
	const unique = new Map(players.map((p) => [p.steamId, p]));
	const groups: SuspectedGroup[] = [];
	for (const faction of [...new Set(factions.filter(Boolean))].sort()) {
		const candidates = [...unique.values()]
			.filter((p) => p.faction === faction && /^\d{17}$/.test(p.steamId))
			.map((p) => ({
				steamId: p.steamId,
				name: p.name,
				prefix: namePrefix(p.name, config.prefixLength)
			}))
			.filter((p) => Array.from(p.prefix).length === config.prefixLength)
			.sort((a, b) => a.prefix.localeCompare(b.prefix) || a.steamId.localeCompare(b.steamId));
		const clusters: GroupMember[][] = [];
		for (const player of candidates) {
			// Every pair must pass: A~B and B~C never imply A~C.
			const cluster = clusters.find((c) =>
				c.every((p) => prefixSimilarity(p.prefix, player.prefix) > config.similarityPercent)
			);
			if (cluster) cluster.push(player);
			else clusters.push([player]);
		}
		for (const members of clusters.filter((c) => c.length >= config.minPlayers)) {
			let minimumSimilarity = 100;
			for (let i = 0; i < members.length; i++)
				for (let j = i + 1; j < members.length; j++)
					minimumSimilarity = Math.min(
						minimumSimilarity,
						prefixSimilarity(members[i].prefix, members[j].prefix)
					);
			groups.push({
				id: `${faction}:${members
					.map((p) => p.steamId)
					.sort()
					.join(',')}`,
				faction,
				minimumSimilarity,
				members
			});
		}
	}
	return groups;
}
