import type { Status } from '$lib/types';
import type { RosterPlayer } from './protocol';

type BattleStatus = Pick<Status, 'serverName' | 'map' | 'matchSeconds' | 'scoreCap' | 'scores'>;
export const battlePageSize = 25;
const clean = (value: string) =>
	Array.from(value.replace(/[\s\x00-\x1f\x7f]+/g, ' ').trim())
		.slice(0, 24)
		.join('');

export function factionBlock(name: string, hex = '') {
	const known: Record<string, string> = {
		RED: '🟥',
		BLU: '🟦',
		GRN: '🟩',
		red: '🟥',
		blue: '🟦',
		green: '🟩'
	};
	if (/^#?[a-f\d]{6}$/i.test(hex)) {
		const rgb = (hex.replace('#', '').match(/../g) || []).map((v) => parseInt(v, 16));
		if (Math.max(...rgb) - Math.min(...rgb) > 30)
			return ['🟥', '🟩', '🟦'][rgb.indexOf(Math.max(...rgb))];
	}
	return known[name] || '⬜';
}

export function battleMessage(status: BattleStatus, roster: RosterPlayer[], page = 1) {
	const scores = status.scores || [];
	const pages = Math.max(1, Math.ceil(roster.length / battlePageSize));
	if (!Number.isInteger(page) || page < 1 || page > pages)
		throw new Error(`玩家名单共有 ${pages} 页，请使用 /局势 1–${pages}。`);
	const cap = Number.isFinite(status.scoreCap) && status.scoreCap! > 0 ? status.scoreCap! : null;
	const clock = status.matchSeconds;
	const elapsed =
		clock !== null && Number.isFinite(clock) && clock >= 0
			? `${Math.floor(clock / 60)}分${Math.floor(clock % 60)}秒`
			: '未知';
	const lines = [
		`${clean(status.serverName)} · ${clean(status.map)}`,
		`已进行 ${elapsed}；在线 ${roster.length} 人`,
		'阵营比分 / 获胜目标（每格10%）'
	];
	for (const team of scores) {
		const block = factionBlock(team.name, team.colorHex);
		const count = roster.filter((p) => p.faction === team.name).length;
		const score = Number.isFinite(team.score) && team.score >= 0 ? team.score : null;
		lines.push(`${block} ${clean(team.name)} · ${count}人 · ${score ?? '未知'} / ${cap ?? '未知'}`);
		if (cap !== null && score !== null) {
			const ratio = Math.min(1, score / cap);
			const filled = Math.floor(ratio * 10);
			lines.push(`${block.repeat(filled)}${'▫️'.repeat(10 - filled)} ${Math.floor(ratio * 100)}%`);
		} else lines.push('胜利进度未知（接口未提供有效比分或目标）');
	}
	if (!scores.length) lines.push('接口暂未提供阵营比分。');
	const ordered = [
		...scores.flatMap((s) => roster.filter((p) => p.faction === s.name)),
		...roster.filter((p) => !scores.some((s) => s.name === p.faction))
	];
	lines.push(`在线玩家 ${page}/${pages}页（每页${battlePageSize}人）`);
	for (const player of ordered.slice((page - 1) * battlePageSize, page * battlePageSize)) {
		const team = scores.find((s) => s.name === player.faction);
		lines.push(
			`${team ? factionBlock(team.name, team.colorHex) : '⬜'} ${clean(player.name)} · ${clean(player.faction || '未入阵营')}`
		);
	}
	if (!roster.length) lines.push('当前无人在线。');
	if (pages > 1) lines.push(`查看其余玩家：/局势 页码（1–${pages}）`);
	return lines.join('\n');
}
