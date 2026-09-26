/** Round-level headshot evidence. No 180-second or historical-baseline sample gate. */
export interface PrecisionCount {
	steamId: string;
	category: string;
	kills: number;
	headshots: number;
}
export interface PrecisionComparison {
	category: string;
	kills: number;
	headshots: number;
	rate: number;
	peerPlayers: number;
	peerKills: number;
	peerHeadshots: number;
	serverRate: number | null;
	difference: number | null;
	ratio: number | null;
	percentile: number | null;
}
/** Other players are the reference: the evaluated player cannot move their own baseline.
 * The population average is kill-weighted; the percentile gives each other player one vote.
 * Equal rates receive mid-rank, so an all-equal server does not become 100th percentile.
 */
export function compareRoundPrecision(
	rows: readonly PrecisionCount[],
	steamId: string
): PrecisionComparison[] {
	const valid = rows.filter(
		(r) =>
			Number.isSafeInteger(r.kills) &&
			r.kills > 0 &&
			Number.isSafeInteger(r.headshots) &&
			r.headshots >= 0 &&
			r.headshots <= r.kills
	);
	const merged = new Map<string, PrecisionCount>();
	for (const row of valid) {
		const key = JSON.stringify([row.steamId, row.category]);
		const previous = merged.get(key);
		if (previous) {
			previous.kills += row.kills;
			previous.headshots += row.headshots;
		} else merged.set(key, { ...row });
	}
	const population = [...merged.values()];
	return population
		.filter((r) => r.steamId === steamId)
		.map((own) => {
			const peers = population.filter((r) => r.steamId !== steamId && r.category === own.category);
			const peerKills = peers.reduce((n, r) => n + r.kills, 0);
			const peerHeadshots = peers.reduce((n, r) => n + r.headshots, 0);
			const rate = own.headshots / own.kills;
			const serverRate = peerKills ? peerHeadshots / peerKills : null;
			// Compare integer cross-products to avoid rounding changing ties.
			const less = peers.filter((r) => r.headshots * own.kills < own.headshots * r.kills).length;
			const equal = peers.filter((r) => r.headshots * own.kills === own.headshots * r.kills).length;
			return {
				category: own.category,
				kills: own.kills,
				headshots: own.headshots,
				rate,
				peerPlayers: peers.length,
				peerKills,
				peerHeadshots,
				serverRate,
				difference: serverRate === null ? null : rate - serverRate,
				ratio: serverRate === null || serverRate === 0 ? null : rate / serverRate,
				percentile: peers.length ? (less + equal * 0.5) / peers.length : null
			};
		});
}

export type PrecisionClass = 'automatic' | 'sniper' | 'shotgun';
export const PRECISION_CLASSES: Readonly<Record<string, PrecisionClass>> = {
	'Id.Item.AK74M': 'automatic',
	'Id.Item.WEPN_029': 'automatic',
	'Id.Item.M4': 'automatic',
	'Id.Item.KH2002': 'automatic',
	'Id.Item.TAR21': 'automatic',
	'Id.Item.A91': 'automatic',
	'Id.Item.MP9': 'automatic',
	'Id.Item.SKS': 'automatic',
	'Id.Item.Mosin': 'sniper',
	'Id.Item.SVDM': 'sniper',
	'Id.Item.SV98': 'sniper',
	'Id.Item.MK22': 'sniper',
	'Id.Item.M500': 'shotgun',
	'Id.Item.MP43': 'shotgun'
};
export const PRECISION_THRESHOLDS = {
	automatic: { watch: 0.6, high: 0.7, label: '步枪／机枪／冲锋枪' },
	sniper: { watch: 0.7, high: 0.9, label: '狙击枪' },
	shotgun: { watch: 0.3, high: 0.5, label: '霰弹枪' }
} as const;
export function precisionDecision(
	row: PrecisionComparison
): 'UNKNOWN' | 'NORMAL' | 'SUSPICIOUS' | 'CHEAT_LIKELY' {
	const threshold = PRECISION_THRESHOLDS[row.category as PrecisionClass];
	if (!threshold || row.kills < 5) return 'UNKNOWN';
	// Cross multiplication preserves inclusive boundaries such as 7/10 and 3/5.
	if (
		row.headshots * 100 >= Math.round(threshold.high * 100) * row.kills ||
		(row.percentile !== null &&
			row.percentile >= 0.95 &&
			row.difference !== null &&
			row.difference > 0)
	)
		return 'CHEAT_LIKELY';
	if (
		row.headshots * 100 >= Math.round(threshold.watch * 100) * row.kills ||
		(row.percentile !== null &&
			row.percentile >= 0.9 &&
			row.difference !== null &&
			row.difference > 0)
	)
		return 'SUSPICIOUS';
	return 'NORMAL';
}
