export type HistoryRetentionPolicy = {
	autoDeleteEnabled: boolean;
	pageSize: number;
	maxRecords: number;
	maxAgeDays: number;
};
export const HISTORY_PAGE_SIZES = [10, 20, 50, 100];
export const HISTORY_RETENTION_DEFAULTS: HistoryRetentionPolicy = {
	autoDeleteEnabled: false,
	pageSize: 20,
	maxRecords: 2000,
	maxAgeDays: 90
};
export function validHistoryPolicy(value: unknown): value is HistoryRetentionPolicy {
	if (!value || typeof value !== 'object') return false;
	const p = value as HistoryRetentionPolicy;
	return (
		typeof p.autoDeleteEnabled === 'boolean' &&
		HISTORY_PAGE_SIZES.includes(p.pageSize) &&
		Number.isInteger(p.maxRecords) &&
		p.maxRecords >= 100 &&
		p.maxRecords <= 100000 &&
		Number.isInteger(p.maxAgeDays) &&
		p.maxAgeDays >= 7 &&
		p.maxAgeDays <= 3650
	);
}
