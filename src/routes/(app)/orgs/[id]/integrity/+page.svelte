<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { toast } from '$lib/toast.svelte';
	import IntegritySettings from '../../../server/[id]/integrity/IntegritySettings.svelte';
	import type { PageProps } from './$types';
	import type { AssessmentMode } from '$lib/server/integrity/statistics';

	let { data }: PageProps = $props();
	let lang = $state<'zh' | 'en'>('zh');
	let busy = $state(false);
	let sourceServer = $state('');
	let importFile = $state<File | null>(null);
	let importPreview = $state<{
		rowCount: number;
		sourceServer: string;
		firstEventAt: string;
		lastEventAt: string;
		weapons: string[];
		maps: string[];
		populationKnown: number;
	} | null>(null);
	async function uploadHistory() {
		if (!importFile || !sourceServer.trim()) {
			toast('请填写来源服务器并选择文件。', 'err');
			return;
		}
		busy = true;
		try {
			const form = new FormData();
			form.set('sourceServer', sourceServer.trim());
			form.set('file', importFile);
			const response = await fetch(
				`/api/orgs/${encodeURIComponent(data.orgId)}/integrity/imports`,
				{
					method: 'POST',
					headers: { 'x-requested-with': 'warcon' },
					body: form,
					credentials: 'same-origin',
					cache: 'no-store'
				}
			);
			const result = await response.json();
			if (!response.ok || !result.ok) throw new Error(result.error?.message ?? '上传失败。');
			importPreview = result.batch;
			toast('文件已暂存，尚未用于统计。', 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function reviewHistory(id: string, decision: 'APPROVED' | 'REJECTED') {
		const label = decision === 'APPROVED' ? '批准' : '撤销／拒绝';
		if (
			!(await confirmDialog(`确认${label}这批外部历史数据？批准后会重新计算统计基线。`, {
				okLabel: `确认${label}`,
				danger: true
			}))
		)
			return;
		busy = true;
		try {
			await api(
				'POST',
				`/api/orgs/${encodeURIComponent(data.orgId)}/integrity/imports/${encodeURIComponent(id)}`,
				{
					decision,
					confirmation: decision === 'APPROVED' ? 'APPROVE_EXTERNAL_INTEGRITY_DATA' : undefined
				}
			);
			toast('审核结果已保存，基线已更新。', 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	// svelte-ignore state_referenced_locally -- editable mode starts with the current owner setting.
	let assessmentMode = $state<AssessmentMode>(data.assessmentMode);
	async function saveMode() {
		if (
			assessmentMode === 'statistical' &&
			!(await confirmDialog(
				'统计模式将接管新的风控判断。请先检查 Baseline 样本、Shadow 对照和差异案件。高置信异常可自动踢出；重复独立案件可在开启对应开关后升级为 24 小时或 7 天临时隔离。确认切换？',
				{ okLabel: '确认切换统计模式', danger: true }
			))
		)
			return;
		busy = true;
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(data.orgId)}/integrity/mode`, {
				mode: assessmentMode,
				confirmation: assessmentMode === 'statistical' ? 'ENABLE_STATISTICAL_INTEGRITY' : undefined
			});
			toast('评估模式已保存。', 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoKickEnabled = $state(data.enforcement.autoKickEnabled);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoQuarantine24hEnabled = $state(data.enforcement.autoQuarantine24hEnabled);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoQuarantine7dEnabled = $state(data.enforcement.autoQuarantine7dEnabled);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoActionMaxPerHour = $state(data.enforcement.autoActionMaxPerHour);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoActionMaxPercentOnline = $state(data.enforcement.autoActionMaxPercentOnline);
	async function save() {
		const enabling =
			(autoKickEnabled && !data.enforcement.autoKickEnabled) ||
			(autoQuarantine24hEnabled && !data.enforcement.autoQuarantine24hEnabled) ||
			(autoQuarantine7dEnabled && !data.enforcement.autoQuarantine7dEnabled);
		if (
			enabling &&
			!(await confirmDialog(
				'实验性自动处置可能误判。启用后系统可能自动踢出、暂时拒入 24 小时或 7 天。不会自动永久封禁。确认开启？',
				{ okLabel: '确认开启实验性自动处置', danger: true }
			))
		)
			return;
		busy = true;
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(data.orgId)}/integrity/enforcement`, {
				values: {
					autoKickEnabled,
					autoQuarantine24hEnabled,
					autoQuarantine7dEnabled,
					autoActionMaxPerHour,
					autoActionMaxPercentOnline,
					confirmation: enabling ? 'ENABLE_EXPERIMENTAL_INTEGRITY' : undefined
				}
			});
			toast('自动处置设置已保存。', 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function resume() {
		if (
			!(await confirmDialog('确认手动恢复实验性自动处置？请先检查熔断原因。', {
				okLabel: '恢复自动处置',
				danger: true
			}))
		)
			return;
		busy = true;
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(data.orgId)}/integrity/enforcement`, {
				values: { resume: true, confirmation: 'ENABLE_EXPERIMENTAL_INTEGRITY' }
			});
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>组织社区风控设置</title></svelte:head>
<div class="mb-5 flex items-center justify-between gap-3">
	<div>
		<h2 class="text-xl font-semibold text-white">
			{lang === 'zh' ? '组织社区风控设置' : 'Organization Community Integrity'}
		</h2>
		<p class="mt-1 text-sm text-mist-400">
			{lang === 'zh'
				? '这些规则适用于本组织的所有服务器。'
				: 'These rules apply to every server in this organization.'}
		</p>
	</div>
	<button class="btn-quiet btn" onclick={() => (lang = lang === 'zh' ? 'en' : 'zh')}
		>{lang === 'zh' ? 'English' : '简体中文'}</button
	>
</div>
<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">
		{lang === 'zh' ? '统计评估迁移' : 'Statistical assessment migration'}
	</h3>
	<p class="mt-2 text-sm text-mist-300">
		{lang === 'zh'
			? '默认 Shadow：同时保存旧评分和真实历史百分位，实际自动处置仍由旧评分决定。统计模式需组织 Owner 明确切换。'
			: 'Shadow is the default: both assessments are saved, while actual enforcement still follows legacy scoring. Only an owner may switch to statistical mode.'}
	</p>
	<div class="mt-3 grid gap-2 text-sm sm:grid-cols-4">
		<div>
			{lang === 'zh' ? '最大基线样本' : 'Largest baseline'}：<strong
				>{data.baselineSummary.maxSamples}</strong
			>
		</div>
		<div>
			{lang === 'zh' ? '可用指标' : 'Available metrics'}：<strong
				>{data.baselineSummary.metrics}/8</strong
			>
			<span class="text-xs text-mist-400">（外服 {data.baselineSummary.externalMetrics}）</span>
		</div>
		<div>
			{lang === 'zh' ? 'Shadow 对照记录' : 'Shadow comparisons'}：<strong
				>{data.comparison.total}</strong
			>
		</div>
		<div>
			{lang === 'zh' ? '对照起点' : 'Comparison since'}：<strong
				>{data.comparison.since
					? new Date(data.comparison.since).toLocaleDateString()
					: '—'}</strong
			>
		</div>
	</div>
	<p class="mt-2 text-xs text-mist-400">
		{lang === 'zh' ? '旧异常／统计正常' : 'Legacy abnormal / statistical normal'}：{data.comparison
			.abnormalNormal} · {lang === 'zh'
			? '旧正常／统计异常'
			: 'Legacy normal / statistical abnormal'}：{data.comparison.normalAbnormal} · {lang === 'zh'
			? '最近基线计算'
			: 'Baseline calculated'}：{data.baselineSummary.calculatedAt
			? new Date(data.baselineSummary.calculatedAt).toLocaleString()
			: '—'}
	</p>
	<div class="mt-4 flex flex-wrap items-end gap-3">
		<label class="block text-sm"
			>{lang === 'zh' ? '评估模式' : 'Assessment mode'}
			<select class="mt-1 input" bind:value={assessmentMode}
				><option value="legacy">{lang === 'zh' ? '旧规则' : 'Legacy'}</option><option
					value="statistical_shadow">{lang === 'zh' ? '统计影子模式' : 'Statistical Shadow'}</option
				><option value="statistical">{lang === 'zh' ? '统计模式' : 'Statistical'}</option></select
			>
		</label><button
			class="btn btn-primary"
			disabled={busy || assessmentMode === data.assessmentMode}
			onclick={saveMode}>{lang === 'zh' ? '保存评估模式' : 'Save assessment mode'}</button
		>
	</div>
	{#if data.baselineSummary.metrics === 0}<p class="mt-2 text-sm text-warn">
			{lang === 'zh'
				? '历史样本不足；切换后统计系统会保持观察，不会因为缺失基线自动处置。'
				: 'Insufficient history: statistical mode will observe, not act, without a baseline.'}
		</p>{/if}
	<p class="mt-2 text-xs text-mist-400">
		{lang === 'zh'
			? '同枪械爆头率与距离按具体武器 cause 分组；距离是历史异常，不代表武器物理射程。本服至少 200 个可比样本，审核后的外服样本至少 30 个可参与实验性统计；自动处置仍要求至少 5000 个可比样本。'
			: 'Weapon cohorts use exact causes. Local baselines need 200 samples; approved external baselines need 30 for experimental review. Automatic action still requires 5000 comparable samples.'}
	</p>
</section>
<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">外服历史数据导入与审核</h3>
	<p class="mt-2 text-sm text-mist-300">
		上传 JSON／JSONL 后先暂存，组织 Owner
		审核批准才会参与统计。外服数据有独立来源标识，不计入实时战绩。
	</p>
	<p class="mt-2 text-sm">
		<a class="text-accent underline" href="/docs/integrity-import" target="_blank" rel="noopener"
			>查看字段模板和逐步导入说明</a
		>
	</p>
	<div class="mt-4 grid gap-3 sm:grid-cols-2">
		<label class="block"
			><span class="field-label">来源服务器标识</span><input
				class="input"
				placeholder="例如 community-eu-01"
				bind:value={sourceServer}
			/></label
		>
		<label class="block"
			><span class="field-label">JSON／JSONL 文件（最多 8 MiB、10,000 条）</span><input
				class="input"
				type="file"
				accept=".json,.jsonl,application/json"
				onchange={(event) => (importFile = event.currentTarget.files?.[0] ?? null)}
			/></label
		>
	</div>
	<button
		class="mt-3 btn btn-primary"
		disabled={busy || !importFile || !sourceServer.trim()}
		onclick={uploadHistory}>上传并暂存</button
	>
	{#if importPreview}<p class="mt-3 text-sm text-mist-200">
			刚上传：{importPreview.sourceServer} · {importPreview.rowCount} 条 · {importPreview.firstEventAt}
			至 {importPreview.lastEventAt} · 已知人数 {importPreview.populationKnown} 条 · 武器 {importPreview.weapons.join(
				'、'
			)} · 地图 {importPreview.maps.join('、')}
		</p>{/if}
	<h4 class="mt-5 font-medium text-white">最近导入批次</h4>
	{#if data.imports.length === 0}<p class="mt-2 text-sm text-mist-400">尚无导入记录。</p>{/if}
	<div class="mt-2 space-y-2">
		{#each data.imports as batch}
			<div class="rounded border border-white/10 p-3 text-sm">
				<div class="flex flex-wrap justify-between gap-2">
					<strong class="text-white">{batch.sourceServer}</strong><span
						>{batch.status === 'STAGED'
							? '待审核'
							: batch.status === 'APPROVED'
								? '已批准'
								: '已拒绝／撤销'}</span
					>
				</div>
				<p class="mt-1 text-mist-400">
					{batch.rowCount} 条 · {new Date(batch.firstEventAt).toLocaleDateString()} 至 {new Date(
						batch.lastEventAt
					).toLocaleDateString()} · SHA-256 {batch.fileSha256.slice(0, 12)}…
				</p>
				{#if batch.status === 'STAGED'}<button
						class="mt-2 btn btn-primary"
						disabled={busy}
						onclick={() => reviewHistory(batch.id, 'APPROVED')}>审核批准</button
					>{/if}
				{#if batch.status !== 'REJECTED'}<button
						class="btn-quiet mt-2 ml-2 btn"
						disabled={busy}
						onclick={() => reviewHistory(batch.id, 'REJECTED')}
						>{batch.status === 'APPROVED' ? '撤销批准' : '拒绝'}</button
					>{/if}
			</div>
		{/each}
	</div>
</section>
<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">
		{lang === 'zh' ? '实验性自动处置' : 'Experimental automatic actions'}
	</h3>
	<p class="my-3 text-sm text-warn">
		{lang === 'zh'
			? '实验性自动处置可能误判。开启后可能自动踢出、24 小时或 7 天暂时拒入；不会自动永久封禁。所有开关默认关闭。'
			: 'Experimental actions can make mistakes. They may kick or temporarily restrict entry for 24 hours or 7 days. Permanent automatic bans are never used. All switches default to off.'}
	</p>
	{#if data.enforcement.autoSuspendedAt}<p class="mb-3 text-sm text-danger">
			{lang === 'zh'
				? '自动处置已熔断，需要管理员手动恢复。'
				: 'Automatic actions are suspended until an owner resumes them.'}
		</p>
		<button class="mb-3 btn btn-danger" disabled={busy} onclick={resume}
			>{lang === 'zh' ? '检查后恢复' : 'Resume after review'}</button
		>{/if}
	<div class="grid gap-3 sm:grid-cols-3">
		<label class="flex items-center gap-2"
			><input type="checkbox" bind:checked={autoKickEnabled} />{lang === 'zh'
				? '自动踢出'
				: 'Automatic kick'}</label
		>
		<label class="flex items-center gap-2"
			><input type="checkbox" bind:checked={autoQuarantine24hEnabled} />{lang === 'zh'
				? '24 小时临时隔离'
				: '24h temporary quarantine'}</label
		>
		<label class="flex items-center gap-2"
			><input type="checkbox" bind:checked={autoQuarantine7dEnabled} />{lang === 'zh'
				? '7 天临时隔离'
				: '7d temporary quarantine'}</label
		>
	</div>
	<div class="mt-4 grid gap-3 sm:grid-cols-2">
		<label class="block"
			><span class="field-label"
				>{lang === 'zh' ? '每小时最多自动处置人数' : 'Maximum actions per hour'}</span
			><input
				class="input"
				type="number"
				min="1"
				max="100"
				bind:value={autoActionMaxPerHour}
			/></label
		>
		<label class="block"
			><span class="field-label"
				>{lang === 'zh' ? '最多占在线人数百分比' : 'Maximum percent of online players'}</span
			><input
				class="input"
				type="number"
				min="1"
				max="100"
				bind:value={autoActionMaxPercentOnline}
			/></label
		>
	</div>
	<button class="mt-4 btn btn-primary" disabled={busy} onclick={save}
		>{lang === 'zh' ? '保存自动处置设置' : 'Save automatic action settings'}</button
	>
</section>
<IntegritySettings
	orgId={data.orgId}
	config={data.ruleConfig}
	ruleDefaults={data.ruleDefaults}
	overrides={data.weaponOverrides}
	weaponDefaults={data.weaponDefaults}
	categories={data.weaponCategories}
	assessmentMode={data.assessmentMode}
	{lang}
/>
