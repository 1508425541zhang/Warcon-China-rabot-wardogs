export const committeeModelName: Record<string, string> = {
	tempo: '击杀节奏',
	precision: '精准度',
	career_deviation: '生涯偏差',
	change_point: '变化点',
	persistence: '持续异常'
};
export const committeeUnknownReason: Record<string, string> = {
	NO_CLEAN_TEMPO_BASELINE: '没有可用的本地击杀节奏基线（至少 50 个参考样本）',
	NO_CLEAN_PRECISION_BASELINE:
		'当前窗口不足 10 次有效步兵击杀，或缺少至少 50 个样本的本地爆头率／穿透率基线',
	INSUFFICIENT_CLEAN_CAREER: '个人独立历史窗口不足 100 个、活跃天数不足 10 天，或历史波动无法估计',
	INSUFFICIENT_ORDERED_HISTORY: '有效有序窗口不足 20 个，或当前统计数据不可用，未加载历史',
	CAREER_VARIANCE_UNRESOLVED: '个人历史变化太小，暂时无法估计变化幅度',
	NO_CURRENT_ASSESSMENT: '当前统计评估尚未就绪',
	UNKNOWN_REASON: '旧记录未保存具体原因'
};
