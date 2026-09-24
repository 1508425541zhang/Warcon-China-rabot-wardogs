/** Exact UI message translations; unknown server or player text is left untouched. */
const MESSAGES: Record<string, string> = {
	Confirm: '确认',
	'Passkey added.': '通行密钥已添加。',
	'Passkey removed.': '通行密钥已移除。',
	'Could not copy; select the text instead.': '复制失败，请手动选中文本。',
	'Password changed. Other sessions were signed out.': '密码已修改，其他会话已退出。',
	'Password removed.': '密码已移除。',
	'Session revoked.': '会话已撤销。',
	'SteamID saved.': 'SteamID 已保存。',
	'SteamID removed.': 'SteamID 已移除。',
	'Authenticator app enrolled.': '身份验证器已启用。',
	'Authenticator app removed.': '身份验证器已移除。',
	'Recovery key discarded.': '恢复密钥已丢弃。',
	'Steam linked. You can sign in with it from now on.': 'Steam 已关联，现在可以用它登录。',
	'Discord linked.': 'Discord 已关联。',
	'That Steam account is already linked to another user.': '该 Steam 账号已关联其他用户。',
	'Steam did not confirm the link. Try again.': 'Steam 未确认关联，请重试。',
	'Organisation created.': '组织已创建。',
	'Organisation renamed.': '组织已重命名。',
	'Organisation deleted.': '组织已删除。',
	'Join code copied.': '加入代码已复制。',
	'Enter a 17-digit SteamID64.': '请输入 17 位 SteamID64。',
	'Sync ran.': '同步已执行。',
	'Ban message saved. Bans placed from now on carry it.':
		'封禁消息已保存，之后的封禁会使用该消息。',
	'Settings saved; the worker picks them up within ten seconds.':
		'设置已保存，后台进程将在 10 秒内读取。',
	'Could not reach the panel. Check your connection.': '无法连接管理面板，请检查网络。',
	'Note added.': '备注已添加。',
	'Note deleted.': '备注已删除。',
	'On the watchlist.': '已加入观察名单。',
	'Removed from the watchlist.': '已移出观察名单。',
	'Steam data refreshed.': 'Steam 数据已刷新。',
	'Admin actions saved.': '管理操作已保存。',
	'Remove:': '移除：',
	'Delete:': '删除：',
	Remove: '移除',
	Delete: '删除',
	Unban: '解除封禁',
	Withdraw: '撤回',
	'Do it': '确认执行',
	Save: '保存',
	Continue: '继续',
	'Risk thresholds must increase from watch to quarantine.': '风险阈值必须从观察到隔离依次递增。',
	'Old Steam ban weights cannot exceed recent weights.': '旧封禁加分不能高于近期封禁加分。',
	'Integrity enforcement is not available in this phase.': '当前阶段尚未开放自动处罚。',
	'Sign-up is not enabled.': '本站未开放注册。',
	'Discord sign-in is not configured.': '尚未配置 Discord 登录。',
	'This panel has not been set up yet. Open /setup first.':
		'面板尚未完成首次设置，请先打开 /setup。',
	'Discord did not return an authorization URL.': 'Discord 未返回授权地址，请重试。',
	'This invite link can no longer be used.': '此邀请链接已无法使用。',
	'This invite link is not valid.': '邀请链接无效。',
	'Setup already completed.': '首次设置已完成。',
	'Setup token is wrong.': '首次设置令牌错误。',
	'Enter the code from your authenticator app.': '请输入验证器应用中的验证码。',
	'That code was not accepted.': '验证码无效。',
	'Username and password are required.': '请输入用户名和密码。',
	'This account is disabled.': '此账号已停用。',
	'Bad username or password.': '用户名或密码错误。',
	'Username and recovery key are required.': '请输入用户名和恢复密钥。',
	'That username and recovery key do not match.': '用户名与恢复密钥不匹配。'
};

const PATTERNS: [RegExp, (...parts: string[]) => string][] = [
	[/^(.+) copied\.$/, (whole, label) => `${label} 已复制。`],
	[/^Testing (.+)…$/, (whole, name) => `正在测试 ${name}…`],
	[/^Ban on (.+) changed\.$/, (whole, name) => `${name} 的封禁已修改。`],
	[/^Access updated for (\d+) members?\.$/, (whole, n) => `已更新 ${n} 位成员的权限。`],
	[/^Saved (\d+) roles?\.$/, (whole, n) => `已保存 ${n} 个角色。`],
	[
		/^Password set\. You can now also sign in as @(.+)\.$/,
		(whole, username) => `密码已设置，现在也可使用 @${username} 登录。`
	],
	[/^Remove the passkey "(.+)"\?$/, (whole, name) => `移除通行密钥“${name}”？`],
	[/^Delete the trigger "(.+)"\?$/, (whole, name) => `删除规则“${name}”？`],
	[/^Delete the '(.+)' role\?$/, (whole, name) => `删除角色“${name}”？`],
	[/^Remove the (.+) webhook\?$/, (whole, name) => `移除 Webhook“${name}”？`],
	[
		/^(.+) must be between ([\d.]+) and ([\d.]+) (ms|days|count|choice)\.$/,
		(whole, key, min, max, unit) =>
			`${key} 必须在 ${min} 到 ${max} ${unit === 'ms' ? '毫秒' : unit === 'days' ? '天' : ''}之间。`
	],
	[
		/^(.+) must be between ([\d.]+) and ([\d.]+)\.$/,
		(whole, key, min, max) => `${key} 必须在 ${min} 到 ${max} 之间。`
	],
	[
		/^(.+) must contain ascending thresholds and weights\.$/,
		(whole, key) => `${key} 的门槛必须递增，权重不能下降。`
	]
];

export function uiMessageZh(message: string): string {
	const exact = MESSAGES[message];
	if (exact) return exact;
	for (const [pattern, render] of PATTERNS) {
		const match = pattern.exec(message);
		if (match) return render(...match);
	}
	return message;
}
