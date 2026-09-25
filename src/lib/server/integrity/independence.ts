/** A saved window with missing or malformed evidence cannot establish an independent episode. */
export function evidenceIds(value: unknown): string[] | null {
	return Array.isArray(value) &&
		value.length > 0 &&
		value.every((id) => typeof id === 'string' && id)
		? value
		: null;
}

export function overlapsEvidence(saved: unknown, current: readonly string[]): boolean {
	const ids = evidenceIds(saved);
	if (!ids || !current.length) return false;
	const set = new Set(ids);
	return current.some((id) => set.has(id));
}

export function independentEvidence(saved: unknown, current: readonly string[]): boolean {
	return !!evidenceIds(saved) && current.length > 0 && !overlapsEvidence(saved, current);
}

/** A separate punishment episode needs distinct kills and a round or time boundary. */
export function independentEpisode(
	saved: { eventIds: unknown; roundId: string | null; clockTo: number; observedAt: Date },
	current: { eventIds: readonly string[]; roundId: string; clockFrom: number; observedAt: Date },
	minimumSeparationSeconds: number
): boolean {
	if (!independentEvidence(saved.eventIds, current.eventIds)) return false;
	if (saved.roundId && saved.roundId !== current.roundId) return true;
	if (saved.roundId === current.roundId)
		return (
			Number.isFinite(saved.clockTo) &&
			current.clockFrom - saved.clockTo >= minimumSeparationSeconds
		);
	return (
		current.observedAt.getTime() - saved.observedAt.getTime() >= minimumSeparationSeconds * 1000
	);
}
