<script lang="ts">
	import { onMount } from 'svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let lang = $state<'zh' | 'en'>('zh');
	onMount(() => {
		const saved = localStorage.getItem('warcon-integrity-lang');
		if (saved === 'en' || saved === 'zh') lang = saved;
	});
	function switchLanguage() {
		lang = lang === 'zh' ? 'en' : 'zh';
		localStorage.setItem('warcon-integrity-lang', lang);
	}
	const words = {
		zh: {
			title: '社区完整性',
			intro: '服务器侧行为风险与证据。分数是审核线索，不代表作弊定论。',
			language: 'English',
			mode: '当前模式',
			dryRun: '仅记录（Dry Run）',
			version: '规则版本',
			feed: '最近击杀事件',
			noFeed: '尚无记录',
			cases: '证据案件',
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
			noActions: '自动踢人和隔离尚未启用。'
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
			noActions: 'Automated kicks and quarantine are not enabled.'
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
</script>

<svelte:head><title>{t.title} · {data.server.name}</title></svelte:head>

<div class="mb-4 flex flex-wrap items-start justify-between gap-3">
	<div>
		<h2 class="text-xl font-semibold text-white">{t.title}</h2>
		<p class="mt-1 text-sm text-mist-400">{t.intro}</p>
	</div>
	<button class="btn-quiet btn" type="button" onclick={switchLanguage}>{t.language}</button>
</div>

<div class="mb-5 grid gap-3 sm:grid-cols-3">
	<div class="panel p-4">
		<div class="label-sm">{t.mode}</div>
		<div class="mt-2 font-medium text-warn">{t.dryRun}</div>
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
	{t.noActions}
</p>

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
							<td>{item.status}</td>
							<td>
								<details>
									<summary class="cursor-pointer">{t.breakdown}</summary>
									<ul class="mt-2 space-y-1 text-xs">
										{#each parts(item.riskBreakdown) as part (part.code)}
											<li>+{part.points} {part.detail}</li>
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
							<td>{item.level}</td>
							<td>v{item.ruleVersion}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">{t.noScores}</p>{/if}
</section>
