<script lang="ts">
	import IntegrityCaseList from '$lib/components/IntegrityCaseList.svelte';
	import { committeeModelName, committeeUnknownReason } from '$lib/committee-display';
	import { onMount } from 'svelte';
	import { refreshVisible } from '$lib/refresh-visible';
	onMount(() => refreshVisible(invalidateAll));
	import { integrityMetricDisplay } from '$lib/integrity-metric-display';
	import { invalidateAll } from '$app/navigation';
	import type { PageProps } from './$types';
	import { integrityCaseStatus, integrityPartText } from '$lib/integrity-display';
	import type { StatisticalAssessment } from '$lib/server/integrity/statistics';
	import DistributionChart from './DistributionChart.svelte';

	let { data }: PageProps = $props();
	// svelte-ignore state_referenced_locally -- initial selection follows the latest saved assessment.
	let selectedSteamId = $state(data.scores.find((score) => score.statistical)?.steamId ?? '');
	let selectedScore = $derived(
		data.scores.find((score) => score.steamId === selectedSteamId && score.statistical)
	);
	let selectedAssessment = $derived(
		(selectedScore?.statistical ?? null) as StatisticalAssessment | null
	);
	let lang = $state<'zh' | 'en'>('zh');

	function switchLanguage() {
		lang = lang === 'zh' ? 'en' : 'zh';
	}
	const words = {
		zh: {
			title: '社区风控',
			intro: '服务器侧行为风险与证据。分数是审核线索，不代表作弊定论。',
			language: 'English',
			mode: '当前模式',
			dryRun: '仅记录（模拟运行）',
			version: '规则版本',
			feed: '最近击杀事件',
			noFeed: '尚无记录',
			cases: '证据案件',
			reports: '近期举报',
			noReports: '暂无举报。',
			reason: '举报原因',
			scores: '近期风险评分',
			noCases: '暂无证据案件。',
			noScores: '暂无异常窗口评分。',
			time: '时间',
			player: '玩家 SteamID64',
			caseId: '案件编号',
			risk: '风险分',
			confidence: '证据等级',
			status: '状态',
			breakdown: '评分依据',
			noActions:
				'Dry Run：不会自动踢出、封禁或隔离玩家。游戏聊天接收与 WARDOGS 官方总游戏时间未接入。',
			dryRunTitle: '历史分数阈值命中统计',
			dryRunHint:
				'按当前阈值比较已保存的历史分数；历史权重版本可能不同。统计不重新评分，也不执行处罚。',
			period: '时间范围',
			windows: '独立异常窗口',
			koPlayers: '达到 KO 阈值的独立玩家',
			quarantinePlayers: '达到隔离阈值的独立玩家',
			contributors: '过去 7 天主要加分规则',
			noContributors: '暂无评分贡献。',
			online: '在线玩家风控',
			noOnline: '当前没有在线玩家快照。',
			refresh: '重新加载已保存数据',
			dataAt: '玩家快照',
			kills: '击杀',
			deaths: '死亡',
			kd: 'KD',
			kpm: '180 秒步兵 KPM',
			peakKpm: '近 10 分钟峰值 KPM',
			victims: '独立受害者',
			level: '风险级别',
			unscored: '未评分',
			metricMissing:
				'≥ 表示仅统计已确认的步兵击杀，仍有阵营或武器待确认；— 表示暂无可显示数据。未评分表示暂无可用信号；已知 VAC、游戏封禁与举报独立计分。',
			truncated: '近期击杀数据超过查询上限，KPM 暂不显示。',
			kdHint:
				'KD 仅作参考，不单独加风险分。风险分由近期行为分项及当前已知 VAC、游戏封禁、24 小时独立举报组成；缺少 KPM 不影响其他分项。'
		},
		en: {
			title: 'Community Integrity',
			intro:
				'Server-side behavior risk and evidence. Scores guide review and do not establish cheating.',
			language: '简体中文',
			mode: 'Current mode',
			dryRun: 'Record only (Dry Run)',
			version: 'Rule version',
			feed: 'Latest kill event',
			noFeed: 'No events recorded',
			cases: 'Evidence cases',
			reports: 'Recent reports',
			noReports: 'No reports yet.',
			reason: 'Reason',
			scores: 'Recent risk scores',
			noCases: 'No evidence cases yet.',
			noScores: 'No abnormal-window scores yet.',
			time: 'Time',
			player: 'Player SteamID64',
			caseId: 'Case ID',
			risk: 'Risk',
			confidence: 'Confidence',
			status: 'Status',
			breakdown: 'Score breakdown',
			noActions:
				'Dry Run: no automatic kick, ban or quarantine. Inbound game chat and official WARDOGS playtime are unavailable.',
			dryRunTitle: 'Historical score threshold counts',
			dryRunHint:
				'Compares saved historical scores with current thresholds. Earlier scoring weights may differ. Scores are not recalculated and no action is taken.',
			period: 'Period',
			windows: 'Distinct abnormal windows',
			koPlayers: 'Unique players at KO threshold',
			quarantinePlayers: 'Unique players at quarantine threshold',
			contributors: 'Top score contributions in 7 days',
			noContributors: 'No score contributions yet.',
			online: 'Online player integrity',
			noOnline: 'No current online roster snapshot.',
			refresh: 'Reload saved data',
			dataAt: 'Roster snapshot',
			kills: 'Kills',
			deaths: 'Deaths',
			kd: 'KD',
			kpm: '180s infantry KPM',
			peakKpm: 'Peak KPM in 10 minutes',
			victims: 'Unique victims',
			level: 'Risk level',
			unscored: 'Not scored',
			metricMissing:
				'≥ counts confirmed infantry kills only; some factions or weapons remain unconfirmed. — means unavailable. Unscored means no rule assessment is recorded yet.',
			truncated: 'Recent kill rows exceeded the query limit; KPM is hidden.',
			kdHint:
				'KD is context only and never adds risk by itself. Risk shows the highest recorded score in the past 15 minutes.'
		}
	};
	let t = $derived(words[lang]);
	const when = (value: string) => new Date(value).toLocaleString(lang === 'zh' ? 'zh-CN' : 'en-US');
	const parts = (value: unknown): { code: string; points: number; detail: string }[] =>
		Array.isArray(value)
			? value.filter(
					(item): item is { code: string; points: number; detail: string } =>
						!!item && typeof item.code === 'string' && typeof item.points === 'number'
				)
			: [];
	const kd = (kills: number, deaths: number) =>
		deaths ? (kills / deaths).toFixed(2) : kills ? '∞' : '—';
	const deliveryLabel = (state: string | null) =>
		(lang === 'zh'
			? {
					pending: '待发送',
					delivered: '已送达',
					failed: '发送失败',
					skipped: '已跳过',
					unknown: '结果未知'
				}
			: {
					pending: 'Pending',
					delivered: 'Delivered',
					failed: 'Failed',
					skipped: 'Skipped',
					unknown: 'Unknown'
				})[(state ?? 'unknown') as 'pending' | 'delivered' | 'failed' | 'skipped' | 'unknown'];
	const actionState = (item: (typeof data.actions)[number]) => {
		if (item.revertedAt) return lang === 'zh' ? '已撤销' : 'Reverted';
		if (item.source === 'REVIEW') {
			const linked = data.reviewPenalties.find((p) => p.id === item.id);
			const active = linked?.active ?? (!item.expiresAt || new Date(item.expiresAt) > new Date());
			return `${active ? '人工封禁有效' : '人工封禁已到期或已撤销'}；即时踢出：${deliveryLabel(item.deliveryState)}`;
		}
		if (item.action === 'KICK') return deliveryLabel(item.deliveryState);
		return `${lang === 'zh' ? '隔离已生效；即时踢出' : 'Quarantine active; immediate kick'}：${deliveryLabel(item.deliveryState)}`;
	};
	const levelName = (value: string | null) => {
		if (!value) return t.unscored;
		if (lang === 'en') return value.replaceAll('_', ' ');
		return (
			(
				{
					NORMAL: '正常',
					PASSIVE_WATCH: '被动观察',
					ACTIVE_WATCH: '主动观察',
					AUTO_KO: '达到移出阈值',
					AUTO_QUARANTINE_ELIGIBLE: '达到隔离资格阈值',
					AUTO_QUARANTINE_24H: '达到 24 小时隔离风险级别',
					AUTO_QUARANTINE_7D: '达到 7 天隔离风险级别'
				} as Record<string, string>
			)[value] ?? value
		);
	};
	let refreshing = $state(false);
	async function refresh() {
		refreshing = true;
		try {
			await invalidateAll();
		} finally {
			refreshing = false;
		}
	}
	let metricsAvailable = $derived(
		data.feedConfigured &&
			!!data.feedAt &&
			Date.now() - new Date(data.feedAt).getTime() < 5 * 60_000 &&
			!data.feedRowsTruncated &&
			data.feedMetricsAvailable
	);
	const signalSources = [
		{
			zh: '180 秒步兵 KPM',
			en: '180s infantry KPM',
			sourceZh: '有效 Kill Feed',
			sourceEn: 'Validated Kill Feed'
		},
		{
			zh: '独立受害者',
			en: 'Unique victims',
			sourceZh: '有效 Kill Feed',
			sourceEn: 'Validated Kill Feed'
		},
		{
			zh: '爆头率',
			en: 'Headshot rate',
			sourceZh: '180 秒有效步兵击杀',
			sourceEn: '180s valid infantry kills'
		},
		{
			zh: '穿透率',
			en: 'Penetration rate',
			sourceZh: 'Kill Feed Penetration 标签',
			sourceEn: 'Kill Feed Penetration tag'
		},
		{ zh: '短时爆发', en: 'Kill burst', sourceZh: '游戏事件时钟', sourceEn: 'Game event clock' },
		{
			zh: 'Steam VAC / 游戏封禁',
			en: 'Steam VAC / game bans',
			sourceZh: '有效 Steam 缓存；无密钥或查询失败时未知',
			sourceEn: 'Valid Steam cache; unknown without key or on failure'
		},
		{
			zh: '重复高风险窗口',
			en: 'Repeated high-risk window',
			sourceZh: '回顾期内独立的高风险证据窗口',
			sourceEn: 'Independent high-risk evidence window in review period'
		},
		{
			zh: '独立举报人数',
			en: 'Unique reporters',
			sourceZh: '24 小时内已验证的 Steam 举报人',
			sourceEn: 'Verified Steam reporters in 24h'
		}
	];
</script>

<svelte:head><title>{t.title} · {data.server.name}</title></svelte:head>

<div class="mb-4 flex flex-wrap items-start justify-between gap-3">
	<div>
		<h2 class="text-xl font-semibold text-white">{t.title}</h2>
		<p class="mt-1 text-sm text-mist-400">{t.intro}</p>
	</div>
	<div class="flex gap-2">
		<a class="btn-quiet btn" href={`/server/${data.server.id}/integrity/ai`}>AI 辅助审核 →</a>
		{#if data.orgIntegrityUrl}<a class="btn-quiet btn" href={data.orgIntegrityUrl}
				>{lang === 'zh' ? '组织风控设置' : 'Organization Integrity settings'}</a
			>{/if}
		<button class="btn-quiet btn" type="button" onclick={switchLanguage}>{t.language}</button>
	</div>
</div>

<div class="mb-5 grid gap-3 sm:grid-cols-3">
	<div class="panel p-4">
		<div class="label-sm">{t.mode}</div>
		<div class="mt-2 font-medium text-warn">
			{data.mode === 'dry_run'
				? t.dryRun
				: data.mode === 'suspended'
					? lang === 'zh'
						? '自动处置已熔断'
						: 'Automatic actions suspended'
					: lang === 'zh'
						? '实验性自动处置'
						: 'Experimental automatic actions'}
		</div>
	</div>
	<div class="panel p-4">
		<div class="label-sm">{t.version}</div>
		<div class="mt-2 font-mono text-lg text-white">v{data.ruleVersion}</div>
	</div>
	<div class="panel p-4">
		<div class="label-sm">{t.feed}</div>
		<div class="mt-2 text-sm text-white">{data.feedAt ? when(data.feedAt) : t.noFeed}</div>
	</div>
</div>

{#if !data.feedConfigured || !data.feedAt}
	<p class="mb-5 rounded-ctl border border-warn/20 bg-warn/5 px-4 py-3 text-sm text-warn">
		{lang === 'zh'
			? data.feedConfigured
				? '已创建 Kill Feed 令牌，但尚未收到游戏服务器的击杀事件。步兵 KPM 暂无可靠数据；请检查服务器配置中的回传地址，并确认游戏服务器能访问该地址。'
				: '此服务器尚未配置 Kill Feed，步兵 KPM 暂无可靠数据。RCON 玩家列表只有累计击杀数，不能推算 180 秒纯步兵 KPM。'
			: data.feedConfigured
				? 'A Kill Feed token exists, but no events have arrived. Infantry KPM is unavailable; check the callback URL and game-server reachability.'
				: 'Kill Feed is not configured. The RCON roster only has cumulative kills and cannot provide a reliable 180-second infantry KPM.'}
		<a class="ml-1 underline" href="/server/{data.server.id}/config">
			{lang === 'zh' ? '查看服务器配置' : 'Open server configuration'}
		</a>
	</p>
{/if}

<p class="mb-5 rounded-ctl border border-warn/20 bg-warn/5 px-4 py-3 text-sm text-warn">
	{lang === 'zh' ? '规则继承自组织。' : 'Rules are inherited from the organization.'}
	{data.mode === 'dry_run'
		? t.noActions
		: lang === 'zh'
			? '自动处置仅在组织管理员显式开启后运行；不会自动永久封禁。'
			: 'Automatic actions run only when enabled by an organization owner; permanent bans are never automatic.'}
</p>

<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">
		{lang === 'zh'
			? '真实历史分布与 Shadow 对照'
			: 'Historical distributions and shadow comparison'}
	</h3>
	<p class="mt-1 text-sm text-mist-400">
		{lang === 'zh'
			? '统计依据过去 30 天真实有效步兵事件；50 个本服样本开始初评，自动踢出仍需 200 个样本及完整保护条件。钟形参考曲线不参与计算。'
			: 'Statistics use 30 days of accepted infantry events. Preliminary review starts at 50 local samples; auto kick requires 200 and all protection gates. The visual bell never drives decisions.'}
	</p>
	<div class="mt-3 flex flex-wrap items-center gap-3">
		<span class="text-sm text-white"
			>{lang === 'zh'
				? '当前评估模式'
				: 'Assessment mode'}：{data.assessmentMode.toUpperCase()}</span
		>
		{#if data.assessmentMode === 'statistical_shadow'}<span class="text-xs text-warn"
				>{lang === 'zh'
					? '实际自动处置仍由旧评分决定'
					: 'Actual enforcement still follows the legacy score'}</span
			>{/if}
		<label class="text-xs text-mist-300"
			>{lang === 'zh' ? '玩家' : 'Player'}
			<select class="ml-2 input" bind:value={selectedSteamId}>
				{#each [...new Set(data.scores
							.filter((score) => score.statistical)
							.map((score) => score.steamId))] as steamId}<option value={steamId}>{steamId}</option
					>{/each}
			</select>
		</label>
	</div>
	{#if selectedAssessment}
		{#if selectedAssessment.sustainedKpm}
			<div class="mt-3 panel p-3 text-sm">
				<strong
					>连续60秒步兵KPM：{selectedAssessment.sustainedKpm.passed ? '达标' : '未达标'}</strong
				>
				<p>
					需要连续 {selectedAssessment.sustainedKpm.requiredMinutes} 个完整分钟，每分钟 KPM ≥ {selectedAssessment
						.sustainedKpm.threshold}；按对局时钟分段，不计算未结束分钟。
				</p>
				<div class="flex flex-wrap gap-3">
					{#each selectedAssessment.sustainedKpm.windows as w}<span
							>{w.from}–{w.to}秒：{w.kpm} KPM · {w.exceeded ? '达标' : '未达标'}</span
						>{/each}
				</div>
				<p>{selectedAssessment.sustainedKpm.reason}。180秒平均KPM仍供展示，不能替代逐分钟条件。</p>
			</div>
		{/if}
		<div class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
			<div class="rounded-ctl border border-white/10 p-3">
				<div class="text-xs text-mist-400">Legacy</div>
				<div class="text-lg text-white">
					{selectedScore?.score ?? 0} · {selectedScore?.level ?? 'NORMAL'}
				</div>
			</div>
			<div class="rounded-ctl border border-white/10 p-3">
				<div class="text-xs text-mist-400">Tempo</div>
				<div class="text-lg text-white">
					{selectedAssessment.tempoPercentile === null
						? '—'
						: `P${(selectedAssessment.tempoPercentile * 100).toFixed(2)}`}
				</div>
			</div>
			<div class="rounded-ctl border border-white/10 p-3">
				<div class="text-xs text-mist-400">Precision</div>
				<div class="text-lg text-white">
					{selectedAssessment.precisionPercentile === null
						? '—'
						: `P${(selectedAssessment.precisionPercentile * 100).toFixed(2)}`}
				</div>
			</div>
			<div class="rounded-ctl border border-white/10 p-3">
				<div class="text-xs text-mist-400">
					{lang === 'zh' ? '统计结果 / 独立事件' : 'Statistical result / episodes'}
				</div>
				<div class="text-lg text-white">
					{selectedAssessment.level ?? 'INSUFFICIENT_DATA'} · {selectedAssessment.independentEpisodes}
				</div>
			</div>
		</div>
		{#if selectedAssessment.status === 'READY'}<div class="mt-4 grid gap-3 lg:grid-cols-2">
				{#each selectedAssessment.metrics as metric (`${metric.code}:${metric.weaponCategory}`)}<DistributionChart
						{metric}
						{lang}
					/>{/each}
			</div>{:else}<p class="mt-4 text-sm text-warn">
				{lang === 'zh'
					? '数据不足：尚无达到 50 个本服可比历史样本的分组。'
					: 'Insufficient data: no local comparable group has 50 historical samples.'}
			</p>{/if}
	{:else}<p class="mt-4 text-sm text-mist-400">
			{lang === 'zh'
				? '尚无统计评估。历史基线由 Worker 定期计算。'
				: 'No statistical assessment yet. The worker builds historical baselines periodically.'}
		</p>{/if}
	<h4 class="mt-6 text-sm font-semibold text-white">
		{lang === 'zh'
			? '过去 30 天：旧系统与统计系统对照'
			: 'Last 30 days: legacy and statistical comparison'}
	</h4>
	<p class="mt-1 text-xs text-mist-400">
		{lang === 'zh' ? '可用评估' : 'Ready assessments'}：{data.comparison.total} · {lang === 'zh'
			? '开始于'
			: 'Since'}：{data.comparison.since ? when(data.comparison.since) : '—'}
	</p>
	<div class="mt-2 table-wrap">
		<table>
			<thead><tr><th></th><th>Statistical Normal</th><th>Statistical Abnormal</th></tr></thead
			><tbody
				><tr
					><th>Legacy Normal</th><td>{data.comparison.normalNormal}</td><td
						>{data.comparison.normalAbnormal}</td
					></tr
				><tr
					><th>Legacy Abnormal</th><td>{data.comparison.abnormalNormal}</td><td
						>{data.comparison.abnormalAbnormal}</td
					></tr
				></tbody
			>
		</table>
	</div>
	<h4 class="mt-6 text-sm font-semibold text-white">
		{lang === 'zh'
			? '委员会 Shadow 汇总（按案件窗口去重）'
			: 'Committee shadow summary (distinct episodes)'}
	</h4>
	<p class="mt-1 text-xs text-mist-400">
		{lang === 'zh'
			? '仅统计已保存的评估；数据不足时不会推断正常。只有统计模式且组织开启自动踢出，满足全部保护条件后才会执行。'
			: 'Only saved assessments are counted. Automatic kicks require statistical mode, organization opt-in and all protection gates.'}
	</p>
	<div class="mt-3 flex flex-wrap gap-4 text-sm text-white">
		<span>NORMAL：{data.committeeShadow.counts.NORMAL}</span>
		<span>WATCH：{data.committeeShadow.counts.WATCH}</span>
		<span>KICK_CANDIDATE：{data.committeeShadow.counts.KICK_CANDIDATE}</span>
		<span
			>{lang === 'zh' ? '意见分歧率' : 'Disagreement'}：{data.committeeShadow.disagreementRate ===
			null
				? '—'
				: `${(data.committeeShadow.disagreementRate * 100).toFixed(1)}%`}</span
		>
		<span
			>{lang === 'zh' ? '候选案件已确认误判' : 'Confirmed false positives'}：{data.committeeShadow
				.falsePositive}</span
		>
		<span
			>{lang === 'zh' ? '候选案件已确认违规' : 'Confirmed abuse'}：{data.committeeShadow
				.confirmedAbuse}</span
		>
	</div>
	{#if data.committeeShadow.truncated}<p class="mt-2 text-xs text-warn">
			{lang === 'zh'
				? '超过 5000 条查询上限，汇总不完整。'
				: 'Over 5000 records; summary is incomplete.'}
		</p>{/if}
	{#if Object.keys(data.committeeShadow.models).length}<div class="mt-3 table-wrap">
			<table>
				<thead
					><tr
						><th>{lang === 'zh' ? '模型' : 'Model'}</th><th>NORMAL</th><th>SUSPICIOUS</th><th
							>CHEAT_LIKELY</th
						><th>{lang === 'zh' ? '未知（未参与判断）' : 'UNKNOWN'}</th><th
							>{lang === 'zh' ? '未知原因 · 记录数' : 'Unknown reasons · count'}</th
						></tr
					></thead
				><tbody>
					{#each Object.entries(data.committeeShadow.models) as [model, votes] (model)}<tr
							><td>{lang === 'zh' ? (committeeModelName[model] ?? model) : model}</td><td
								>{votes.NORMAL}</td
							><td>{votes.SUSPICIOUS}</td><td>{votes.CHEAT_LIKELY}</td><td>{votes.UNKNOWN}</td><td
								class="max-w-md text-xs text-mist-400"
							>
								{#each Object.entries(data.committeeShadow.unknownReasons[model] ?? {}) as [reason, count] (reason)}
									<div>
										{lang === 'zh' ? (committeeUnknownReason[reason] ?? reason) : reason} · {count}
									</div>
								{:else}—{/each}
							</td></tr
						>{/each}
				</tbody>
			</table>
		</div>{/if}
</section>

<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">
		{lang === 'zh' ? '信号接入状态' : 'Signal sources'}
	</h3>
	<div class="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
		{#each signalSources as signal (signal.en)}
			<div class="rounded-ctl border border-white/10 p-3">
				<div class="text-sm font-medium text-white">{lang === 'zh' ? signal.zh : signal.en}</div>
				<div class="mt-1 text-xs text-mist-400">
					{lang === 'zh' ? signal.sourceZh : signal.sourceEn}
				</div>
			</div>
		{/each}
	</div>
	<p class="mt-3 text-xs text-mist-400">
		{lang === 'zh'
			? '未接入：WARDOGS 官方总游戏时间、游戏内聊天接收。缺少可靠 Feed 时，实时行为指标显示为未知。'
			: 'Unavailable: official WARDOGS playtime and inbound game chat. Live behavior metrics are unknown without a reliable feed.'}
	</p>
</section>

<section class="mb-6 panel p-4">
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h3 class="text-base font-semibold text-white">{t.online}</h3>
		<button class="btn-quiet btn" type="button" disabled={refreshing} onclick={refresh}
			>{t.refresh}</button
		>
	</div>
	<p class="mt-1 mb-3 text-xs text-mist-400">
		{t.dataAt}: {data.playersAt ? when(data.playersAt) : t.noFeed} · {t.kdHint}
		<br />{t.metricMissing}
	</p>
	{#if data.feedRowsTruncated}<p class="mb-2 text-sm text-warn">{t.truncated}</p>{/if}
	{#if data.onlinePlayers.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.player}</th><th>{t.kills}</th><th>{t.deaths}</th><th>{t.kd}</th><th>{t.kpm}</th
						><th>{lang === 'zh' ? '实时步兵 KPM（60 秒）' : 'Infantry KPM (60s)'}</th><th
							>{t.peakKpm}</th
						><th>{t.victims}</th><th>{t.risk}</th><th>{t.level}</th><th>{t.breakdown}</th></tr
					></thead
				>
				<tbody
					>{#each data.onlinePlayers as player (player.steamId)}
						<tr>
							<td
								><a class="text-accent" href="/server/{data.server.id}/players/{player.steamId}"
									>{player.name}</a
								>
								<div class="font-mono text-xs text-mist-400">{player.steamId}</div></td
							>
							<td>{player.kills}</td><td>{player.deaths}</td><td
								>{kd(player.kills, player.deaths)}</td
							>
							<td
								title={!metricsAvailable || player.infantry?.reliable === false
									? t.metricMissing
									: undefined}
								>{integrityMetricDisplay(
									player.infantry?.kpm180 ?? 0,
									metricsAvailable,
									player.infantry?.reliable180 !== false
								)}</td
							>
							<td
								>{integrityMetricDisplay(
									player.infantry?.kpm60 ?? 0,
									metricsAvailable,
									player.infantry?.reliable60 !== false
								)}</td
							>
							<td
								>{integrityMetricDisplay(
									player.infantry?.peakKpm180 ?? 0,
									metricsAvailable,
									player.infantry?.reliable !== false
								)}</td
							>
							<td
								>{integrityMetricDisplay(
									player.infantry?.uniqueVictims180 ?? 0,
									metricsAvailable,
									player.infantry?.reliable !== false,
									0
								)}</td
							>
							<td>{player.riskScore ?? '—'}</td><td>{levelName(player.riskLevel)}</td>
							<td
								>{#if parts(player.riskBreakdown).length}<details>
										<summary class="cursor-pointer">{t.breakdown}</summary>
										<ul class="mt-2 space-y-1 text-xs">
											{#each parts(player.riskBreakdown) as part (part.code)}<li>
													+{part.points}
													{integrityPartText(part.code, part.detail, lang)}
												</li>{/each}
										</ul>
									</details>{:else}—{/if}</td
							>
						</tr>
					{/each}</tbody
				>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">{t.noOnline}</p>{/if}
</section>

<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">{t.dryRunTitle}</h3>
	<p class="mt-1 mb-3 text-xs text-mist-400">{t.dryRunHint}</p>
	<div class="table-wrap">
		<table>
			<thead
				><tr
					><th>{t.period}</th><th>{t.windows}</th><th>{t.koPlayers}</th><th
						>{t.quarantinePlayers}</th
					></tr
				></thead
			>
			<tbody>
				{#each data.dryRun as item (item.hours)}
					<tr
						><td>{item.hours === 168 ? '7 d' : `${item.hours} h`}</td><td>{item.windows}</td><td
							>{item.koPlayers}</td
						><td>{item.quarantinePlayers}</td></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
	<h4 class="mt-4 text-sm font-semibold text-white">{t.contributors}</h4>
	{#if data.contributors.length}
		<ul class="mt-2 space-y-1 text-sm text-mist-300">
			{#each data.contributors as item (item.code)}<li>{item.code}: +{item.points}</li>{/each}
		</ul>
	{:else}<p class="mt-2 text-sm text-mist-400">{t.noContributors}</p>{/if}
</section>

<section class="mb-6 panel p-4">
	<h3 class="mb-3 text-base font-semibold text-white">
		{lang === 'zh' ? '处罚记录（自动／人工）' : 'Actions (automatic / human review)'}
	</h3>
	{#if data.actions.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.time}</th><th>{t.player}</th><th>{t.caseId}</th><th
							>{lang === 'zh' ? '处置' : 'Action'}</th
						><th>{lang === 'zh' ? '执行状态' : 'Delivery'}</th><th
							>{lang === 'zh' ? '生效时间' : 'Effective at'}</th
						><th>{lang === 'zh' ? '到期' : 'Expires'}</th></tr
					></thead
				>
				<tbody
					>{#each data.actions as item (item.id)}<tr
							><td>{when(item.createdAt)}</td><td class="font-mono">{item.steamId}</td><td
								class="font-mono">{item.caseId}</td
							><td
								>{item.source === 'REVIEW'
									? '人工确认违规（7天；保留更长期封禁）'
									: item.action}</td
							><td>{actionState(item)}</td><td>{item.effectiveAt ? when(item.effectiveAt) : '—'}</td
							><td>{item.expiresAt ? when(item.expiresAt) : '—'}</td></tr
						>{/each}</tbody
				>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">
			{lang === 'zh' ? '暂无处罚记录。' : 'No actions.'}
		</p>{/if}
</section>

<IntegrityCaseList {data} {lang} archiveUrl={`/server/${data.server.id}/integrity/cases`} />

<section class="mb-6 panel p-4">
	<h3 class="mb-3 text-base font-semibold text-white">{t.reports}</h3>
	{#if data.reports.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr><th>{t.time}</th><th>{t.player}</th><th>{t.reason}</th><th>{t.status}</th></tr
					></thead
				>
				<tbody>
					{#each data.reports as item (item.id)}
						<tr>
							<td class="whitespace-nowrap">{when(item.createdAt)}</td>
							<td class="font-mono">{item.targetSteamId}</td>
							<td>{item.reason}</td>
							<td>{lang === 'zh' ? integrityCaseStatus(item.status) : item.status}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">{t.noReports}</p>{/if}
</section>

<section class="panel p-4">
	<h3 class="mb-3 text-base font-semibold text-white">{t.scores}</h3>
	{#if data.scores.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.time}</th><th>{t.player}</th><th>{t.risk}</th><th>{t.status}</th><th
							>{t.version}</th
						></tr
					></thead
				>
				<tbody>
					{#each data.scores as item (item.id)}
						<tr>
							<td class="whitespace-nowrap">{when(item.scoredAt)}</td>
							<td class="font-mono">{item.steamId}</td>
							<td>{item.score}</td>
							<td>{levelName(item.level)}</td>
							<td>v{item.ruleVersion}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">{t.noScores}</p>{/if}
</section>
