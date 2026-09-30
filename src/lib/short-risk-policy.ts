export const SHORT_RISK_SHA = 'b4b68f4dc09b95d24e44d3ab70322f46fc78d50980d44a8e86409ace734649cc';
export const SHORT_WARNING = 0.6848768837889491;
export const SHORT_KICK = 0.6939192028934524;
export type ShortWindow = { at: string; score: number };
export function shortWindowConsensus(windows: ShortWindow[] | undefined, evaluatedAt: string) {
	if (!Array.isArray(windows) || windows.length !== 5) return null;
	const end = Date.parse(evaluatedAt);
	if (!Number.isFinite(end) || Date.parse(windows[4].at) !== end) return null;
	let total = 0;
	for (let i = 0; i < 5; i++) {
		const at = Date.parse(windows[i].at);
		if (
			!Number.isFinite(at) ||
			!Number.isFinite(windows[i].score) ||
			windows[i].score <= SHORT_KICK
		)
			return null;
		if (
			i &&
			(at - Date.parse(windows[i - 1].at) < 8000 || at - Date.parse(windows[i - 1].at) > 20000)
		)
			return null;
		total += windows[i].score * (i + 1);
	}
	const weighted = total / 15;
	return weighted > SHORT_KICK ? weighted : null;
}
export function shortRiskDecision(score: number) {
	if (!Number.isFinite(score)) return 'unavailable';
	return score > SHORT_KICK ? 'kick' : score > SHORT_WARNING ? 'warning' : 'normal';
}
export function freshShortSnapshot(
	x: { evaluated_at: string; model_sha256: string; algorithm: string; players: unknown },
	now = Date.now()
) {
	const age = now - Date.parse(x.evaluated_at);
	return (
		x.model_sha256 === SHORT_RISK_SHA &&
		x.algorithm === 'IsolationForest' &&
		Number.isFinite(age) &&
		age >= 0 &&
		age <= 30000 &&
		Array.isArray(x.players)
	);
}
