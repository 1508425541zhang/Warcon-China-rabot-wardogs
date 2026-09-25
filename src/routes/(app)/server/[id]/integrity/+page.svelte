<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import type { PageProps } from './$types';
	import { integrityCaseStatus, integrityPartText } from '$lib/integrity-display';

	let { data }: PageProps = $props();
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
			dryRunTitle: '模拟运行影响预览',
			dryRunHint:
				'按当前阈值统计已记录的异常窗口评分；历史权重版本可能不同，不等同于规则回放。不会执行处罚。',
			period: '时间范围',
			windows: '异常窗口',
			koPlayers: '达到 KO 阈值的独立玩家',
			quarantinePlayers: '达到隔离阈值的独立玩家',
			contributors: '过去 7 天主要加分规则',
			noContributors: '暂无评分贡献。',
			online: '在线玩家风控',
			noOnline: '当前没有在线玩家快照。',
			refresh: '刷新数据',
			dataAt: '玩家快照',
			kills: '击杀',
			deaths: '死亡',
			kd: 'KD',
			kpm: '180 秒步兵 KPM',
			peakKpm: '近 10 分钟峰值 KPM',
			victims: '独立受害者',
			level: '风险级别',
			unscored: '未评分',
			metricMissing: '暂无可靠击杀数据',
			truncated: '近期击杀数据超过查询上限，KPM 暂不显示。',
			kdHint: 'KD 仅作参考，不单独加风险分。风险分显示过去 15 分钟内的最高记录。'
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
			dryRunTitle: 'Dry Run impact preview',
			dryRunHint:
				'Counts recorded abnormal-window scores against current thresholds. Historical weight versions may differ; this is not rule replay. No action is taken.',
			period: 'Period',
			windows: 'Abnormal windows',
			koPlayers: 'Unique players at KO threshold',
			quarantinePlayers: 'Unique players at quarantine threshold',
			contributors: 'Top score contributions in 7 days',
			noContributors: 'No score contributions yet.',
			online: 'Online player integrity',
			noOnline: 'No current online roster snapshot.',
			refresh: 'Refresh',
			dataAt: 'Roster snapshot',
			kills: 'Kills',
			deaths: 'Deaths',
			kd: 'KD',
			kpm: '180s infantry KPM',
			peakKpm: 'Peak KPM in 10 minutes',
			victims: 'Unique victims',
			level: 'Risk level',
			unscored: 'Not scored',
			metricMissing: 'No reliable recent kill data',
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
	const caseSignals = (snapshot: unknown): string => {
		if (!snapshot || typeof snapshot !== 'object') return '';
		const data = snapshot as Record<string, unknown>;
		const names: Record<string, [string, string]> = {
			kpm: ['步兵 KPM', 'Infantry KPM'],
			headshot: ['爆头率', 'Headshot rate'],
			penetration: ['穿透率', 'Penetration rate'],
			burst: ['短时爆发', 'Kill burst']
		};
		return Array.isArray(data.behaviorReasons)
			? data.behaviorReasons
					.filter((reason): reason is string => typeof reason === 'string')
					.map((reason) => names[reason]?.[lang === 'zh' ? 0 : 1] ?? reason)
					.join(' · ')
			: '';
	};
	const kd = (kills: number, deaths: number) =>
		deaths ? (kills / deaths).toFixed(2) : kills ? '∞' : '—';
	const deliveryLabel = (state: string | null) =>
		(lang === 'zh'
			? { pending: '待发送', delivered: '已送达', failed: '发送失败', skipped: '已跳过', unknown: '结果未知' }
			: { pending: 'Pending', delivered: 'Delivered', failed: 'Failed', skipped: 'Skipped', unknown: 'Unknown' }
		)[(state ?? 'unknown') as 'pending' | 'delivered' | 'failed' | 'skipped' | 'unknown'];
	const actionState = (item: (typeof data.actions)[number]) => {
		if (item.revertedAt) return lang === 'zh' ? '已撤销' : 'Reverted';
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
		!!data.feedAt &&
			Date.now() - new Date(data.feedAt).getTime() < 5 * 60_000 &&
			!data.feedRowsTruncated
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
	</p>
	{#if data.feedRowsTruncated}<p class="mb-2 text-sm text-warn">{t.truncated}</p>{/if}
	{#if data.onlinePlayers.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.player}</th><th>{t.kills}</th><th>{t.deaths}</th><th>{t.kd}</th><th>{t.kpm}</th
						><th>{t.peakKpm}</th><th>{t.victims}</th><th>{t.risk}</th><th>{t.level}</th><th
							>{t.breakdown}</th
						></tr
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
							<td title={!metricsAvailable ? t.metricMissing : undefined}
								>{metricsAvailable ? (player.infantry?.kpm180 ?? 0).toFixed(2) : '—'}</td
							>
							<td>{metricsAvailable ? (player.infantry?.peakKpm180 ?? 0).toFixed(2) : '—'}</td>
							<td>{metricsAvailable ? (player.infantry?.uniqueVictims180 ?? 0) : '—'}</td>
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
		{lang === 'zh' ? '自动处置记录' : 'Automatic actions'}
	</h3>
	{#if data.actions.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.time}</th><th>{t.player}</th><th>{t.caseId}</th><th
							>{lang === 'zh' ? '处置' : 'Action'}</th
						><th>{lang === 'zh' ? '执行状态' : 'Delivery'}</th><th>{lang === 'zh' ? '生效时间' : 'Effective at'}</th><th>{lang === 'zh' ? '到期' : 'Expires'}</th></tr
					></thead
				>
				<tbody
					>{#each data.actions as item (item.id)}<tr
							><td>{when(item.createdAt)}</td><td class="font-mono">{item.steamId}</td><td
								class="font-mono">{item.caseId}</td
							><td>{item.action}</td><td>{actionState(item)}</td><td>{item.effectiveAt ? when(item.effectiveAt) : '—'}</td><td>{item.expiresAt ? when(item.expiresAt) : '—'}</td></tr
						>{/each}</tbody
				>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">
			{lang === 'zh' ? '暂无自动处置。' : 'No automatic actions.'}
		</p>{/if}
</section>

<section class="mb-6 panel p-4">
	<h3 class="mb-3 text-base font-semibold text-white">{t.cases}</h3>
	{#if data.cases.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.time}</th><th>{t.player}</th><th>{t.caseId}</th><th>{t.risk}</th><th
							>{t.confidence}</th
						><th>{t.status}</th><th>{t.breakdown}</th></tr
					></thead
				>
				<tbody>
					{#each data.cases as item (item.id)}
						<tr>
							<td class="whitespace-nowrap">{when(item.createdAt)}</td>
							<td
								><a
									class="font-mono text-accent"
									href="/server/{data.server.id}/players/{item.steamId}">{item.steamId}</a
								></td
							>
							<td class="font-mono">{item.id}</td>
							<td>{item.riskScore}</td>
							<td>{item.confidence}</td>
							<td>{lang === 'zh' ? integrityCaseStatus(item.status) : item.status}</td>
							<td>
								<details>
									<summary class="cursor-pointer">{t.breakdown}</summary>
									{#if caseSignals(item.snapshot)}<p class="mt-2 text-xs text-accent">
											{caseSignals(item.snapshot)}
										</p>{/if}
									<ul class="mt-2 space-y-1 text-xs">
										{#each parts(item.riskBreakdown) as part (part.code)}
											<li>+{part.points} {integrityPartText(part.code, part.detail, lang)}</li>
										{/each}
									</ul>
								</details>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">{t.noCases}</p>{/if}
</section>

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
