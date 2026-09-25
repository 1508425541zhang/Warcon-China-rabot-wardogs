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
