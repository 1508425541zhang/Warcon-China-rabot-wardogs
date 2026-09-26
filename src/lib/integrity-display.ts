/** Human-readable labels for stored Integrity evidence, including older English snapshots. */
const RISK_LABELS: Record<string, string> = {
	infantry_kpm_180: '180 秒步兵 KPM',
	unique_victims: '独立受害者',
	repeat_window: '独立异常窗口',
	repeat_extreme: '重复极高 KPM',
	unique_reports: '独立举报人',
	repeat_auto_ko: '重复高风险窗口', // Historic snapshots retain the old component code.
	repeat_high_risk_window: '重复高风险窗口',
	headshots: '步兵爆头',
	penetrations: '穿透击杀',
	kill_burst: '短时间击杀',
	steam_ban_prior: '公开 Steam 封禁记录',
	low_playtime: '低公开游戏时间与极高 KPM'
};

export function integrityPartText(code: string, detail: string, lang: 'zh' | 'en' = 'zh') {
	if (lang === 'en') return detail;
	const label = RISK_LABELS[code] ?? code;
	const numbers = detail.match(/\d+(?:\.\d+)?/g) ?? [];
	if (code === 'infantry_kpm_180' && numbers[0]) return `${label} ${numbers[0]}`;
	if (code === 'unique_victims' && numbers[0]) return `${label} ${numbers[0]} 人`;
	if (code === 'unique_reports' && numbers[0]) return `${label} ${numbers[0]} 人`;
	if (code === 'repeat_window' && numbers[0]) return `${label} ${numbers[0]} 次`;
	if (code === 'repeat_extreme' && numbers[0]) return `${label}：两个窗口均达到 ${numbers[0]} KPM`;
	if ((code === 'headshots' || code === 'penetrations') && numbers.length >= 2)
		return `${label} ${numbers[0]} / ${numbers[1]}`;
	return label;
}

export const integrityCaseStatus = (value: string) =>
	(
		({ OPEN: '待审核', REVIEWED: '已审核', REVIEWING: '审核中', CLOSED: '已结案' }) as Record<
			string,
			string
		>
	)[value] ?? value;

/** Render legacy advisory risk reasons without changing stored evidence or scoring. */
export function legacyRiskReasonZh(code: string, text: string): string {
	const numbers = text.match(/\d+(?:\.\d+)?/g) ?? [];
	switch (code) {
		case 'vac':
			return `${numbers[0] ?? '—'} 次 VAC 封禁${numbers[1] ? `；最近一次在 ${numbers[1]} 天前` : ''}`;
		case 'gameban':
			return `${numbers[0] ?? '—'} 次游戏封禁${numbers[1] ? `；最近一次在 ${numbers[1]} 天前` : ''}`;
		case 'community':
			return 'Steam 社区封禁';
		case 'economy': {
			const state = text.match(/\(([^)]+)\)/)?.[1];
			return `Steam 交易封禁${state ? `（${state}）` : ''}`;
		}
		case 'private':
			return 'Steam 资料不公开';
		case 'age':
			return `Steam 账号注册 ${numbers[0] ?? '—'} 天`;
		case 'friends_private':
			return 'Steam 好友列表不公开';
		case 'banned_friends':
			return `已检查 ${numbers[1] ?? '—'} 位 Steam 好友，其中 ${numbers[0] ?? '—'} 位有封禁记录${numbers[2] ? `（总数 ${numbers[2]} 位）` : ''}`;
		case 'win_rate':
			return `已记录的 ${numbers[1] ?? '—'} 场对局中，胜率为 ${numbers[0] ?? '—'}%`;
		case 'kd':
			return `${numbers[1] ?? '—'} 次击杀、${numbers[2] ?? '—'} 次死亡，KD ${numbers[0] ?? '—'}`;
		case 'headshots':
			return `${numbers[1] ?? '—'} 次击杀事件中，爆头率为 ${numbers[0] ?? '—'}%`;
		case 'banned_elsewhere':
			return text.replace(/^Banned on /, '已在以下服务器封禁：');
		case 'resembles':
			return text
				.replace(/^Name resembles banned /, '昵称与已封禁玩家相似：')
				.replace(/ on ([^()]+)$/, '；服务器：$1');
		case 'watchlist':
			return text.replace(/^On the watchlist/, '已在关注名单');
		default:
			return text;
	}
}
