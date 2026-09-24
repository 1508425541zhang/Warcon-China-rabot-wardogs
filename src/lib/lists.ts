// Browser-side helpers for the organisation ban and reserved-slot lists.
import type { Tone } from './components/Badge.svelte';
import type { ListEntryState, ListKind, ListSyncSummary } from './types';

export const KIND_TITLE: Record<ListKind, string> = { ban: '封禁名单', reserve: '预留位' };

export const STATE_TONE: Record<ListEntryState, Tone> = {
	applied: 'ok',
	failed: 'err',
	pending: 'warn',
	local: ''
};

export const STATE_TEXT: Record<ListEntryState, string> = {
	applied: '面板已应用',
	failed: '应用失败',
	pending: '等待下次同步',
	local: '服务器上已有，非面板添加'
};

export const REASON_PRESETS = [
	'作弊',
	'误杀队友',
	'恶意言行',
	'种族歧视／仇恨言论',
	'规避封禁',
	'恶意破坏'
];

/** value = days; 0 = permanent; 'custom' = a datetime-local input */
export const EXPIRY_OPTIONS = [
	['0', '永久'],
	['1', '1 天'],
	['7', '7 天'],
	['30', '30 天'],
	['custom', '指定日期…']
] as const;

/** The ISO timestamp an expiry choice stands for, or null for permanent. */
export function expiryIso(choice: string, custom: string): string | null {
	if (choice === 'custom') return custom ? new Date(custom).toISOString() : null;
	const days = Number(choice);
	return days > 0 ? new Date(Date.now() + days * 86400_000).toISOString() : null;
}

/** One line for a toast: where a list change landed. */
export function describeSync(sync: ListSyncSummary, done: string): string {
	const s = sync.servers;
	const action = done.replace(/[。.!！]+$/, '');
	if (!s.length) return `${action}；服务器会在下次轮询时同步。`;
	const applied = s.filter((x) => x.ok && !x.failed).length;
	const parts = [`${action}；已在 ${applied} / ${s.length} 台服务器应用。`];
	const pending = s.filter((x) => x.pending).map((x) => x.serverName);
	const down = s.filter((x) => !x.ok && !x.pending).map((x) => x.serverName);
	const failed = s.filter((x) => x.ok && x.failed).map((x) => x.serverName);
	if (pending.length) parts.push(`仍在同步：${pending.join('、')}。`);
	if (down.length) parts.push(`暂时无法连接，稍后重试：${down.join('、')}。`);
	if (failed.length) parts.push(`服务器拒绝：${failed.join('、')}（请查看名单页面）。`);
	return parts.join(' ');
}
