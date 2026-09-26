export interface CashObservation {
	observedAt: string;
	players: { steamId: string; cash: number }[];
}
export interface CashPoint {
	seconds: number;
	cash: number;
	growth: number;
	serverGrowth: number;
	peers: number;
}
/** Net change from each player's first recorded cash. Spending may reduce it. Never fabricate missing XP. */
export function cashProgress(
	samples: CashObservation[],
	steamId: string,
	startedAt: string
): CashPoint[] {
	const initial = new Map<string, number>();
	const points: CashPoint[] = [];
	for (const sample of [...samples].sort(
		(a, b) => Date.parse(a.observedAt) - Date.parse(b.observedAt)
	)) {
		const players = sample.players.filter((p) => Number.isFinite(p.cash));
		for (const p of players) if (!initial.has(p.steamId)) initial.set(p.steamId, p.cash);
		const own = players.find((p) => p.steamId === steamId);
		if (!own) continue;
		const growth = players.reduce((sum, p) => sum + p.cash - initial.get(p.steamId)!, 0);
		points.push({
			seconds: Math.max(0, (Date.parse(sample.observedAt) - Date.parse(startedAt)) / 1000),
			cash: own.cash,
			growth: own.cash - initial.get(steamId)!,
			serverGrowth: growth / players.length,
			peers: players.length
		});
	}
	return points;
}
