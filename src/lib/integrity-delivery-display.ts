/** Explain stored outcomes without misrepresenting a requested action as delivered. */
export function integrityDeliveryReason(reason: string, lang: 'zh' | 'en'): string {
	if (!reason) return lang === 'zh' ? '未记录执行详情' : 'No delivery detail recorded';
	if (lang === 'en') return reason;
	const known: Record<string, string> = {
		'Player already left.': '执行前玩家已离线，即时踢出未发送；已生效的封禁不受影响。',
		'Integrity action or case changed before delivery':
			'执行前案件或处置状态检查未通过，未发送踢出。旧版本的 AUTO_ACTION 状态冲突也会产生此记录。',
		'Integrity enforcement disabled before delivery':
			'执行前规则、开关或统计模型条件已变化，保护检查阻止执行。',
		'Server no longer polled.': '服务器已停止轮询，无法执行。',
		'Integrity delivery validation failed': '执行前校验发生错误，未确认执行。'
	};
	const stale = reason.match(/^Stale \((\d+)s old\)\.$/);
	return known[reason] ?? (stale ? `指令已等待 ${stale[1]} 秒，超过有效期，未发送。` : reason);
}
