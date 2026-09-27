<script lang="ts">
	import AiCaseSummary from '$lib/components/AiCaseSummary.svelte';
	import { onMount } from 'svelte';
	import { refreshVisible } from '$lib/refresh-visible';

	import { api, errorMessage } from '$lib/api';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	let loadedServer = $state(''),
		autoEnabled = $state(true),
		autoCloseEnabled = $state(true),
		deleteLowRisk = $state(true),
		dailyLimit = $state(100);
	onMount(() => refreshVisible(invalidateAll));
	let baseUrl = $state(''),
		model = $state(''),
		apiKey = $state(''),
		maxTokens = $state(1200),
		tokenParameter = $state('max_tokens');
	let models = $state<string[]>([]),
		busy = $state(false),
		notice = $state(''),
		failure = $state('');
	let review = $state<{
		verdict: string;
		summary: string;
		reasons: { text: string; evidence: string }[];
		contradictions: string[];
		missingEvidence: string[];
		model: string;
	} | null>(null);
	let caseId = $state(''),
		preview = $state(''),
		result = $state('');
	$effect(() => {
		if (data.config && loadedServer !== data.server.id) {
			loadedServer = data.server.id;
			autoEnabled = data.config.autoEnabled;
			autoCloseEnabled = data.config.autoCloseEnabled;
			deleteLowRisk = data.config.deleteLowRisk;
			dailyLimit = data.config.dailyLimit;
			baseUrl = data.config.baseUrl;
			model = data.config.model;
			maxTokens = data.config.maxTokens;
			tokenParameter = data.config.tokenParameter;
		}
	});
	const endpoint = () => `/api/servers/${page.params.id}/integrity/ai`;
	async function run(operation: string) {
		busy = true;
		notice = '';
		failure = '';
		try {
			const response = await api('POST', endpoint(), {
				operation,
				caseId,
				settings:
					operation === 'save'
						? {
								baseUrl,
								model,
								apiKey,
								maxTokens,
								tokenParameter,
								autoEnabled,
								autoCloseEnabled,
								deleteLowRisk,
								dailyLimit
							}
						: undefined
			});
			if (operation === 'save') {
				apiKey = '';
				await invalidateAll();
				notice = '已保存，密钥已加密。';
			}
			if (operation === 'delete') {
				apiKey = '';
				baseUrl = '';
				model = '';
				await invalidateAll();
				notice = '已删除接口配置，已停用 AI 请求。';
			}
			if (operation === 'models') {
				models = response.models;
				notice = models.length
					? '模型列表已读取，选择后请保存。'
					: '接口未提供模型列表，请手动填写模型名称。';
			}
			if (operation === 'test') notice = response.message;
			if (operation === 'preview') preview = JSON.stringify(response.bundle, null, 2);
			if (operation === 'review') {
				review = response.result;
				result = JSON.stringify(response.result, null, 2);
				notice = response.cached
					? '已读取缓存，没有再次调用模型。'
					: '辅助审核完成，人工审核状态未改变。';
			}
		} catch (e) {
			failure = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>AI 辅助审核</title></svelte:head>
<a class="text-sm text-mist-400" href={`/server/${page.params.id}/integrity`}>← 返回社区风控</a>
<h2 class="my-4 text-xl font-semibold">AI 辅助审核</h2>
<p class="mb-4 text-sm text-mist-400">
	新案件后台自动初审；未达到委员会正式建案门槛时，只要一位专家可疑或极可能作弊，也会自动生成 AI
	预筛记录并排队。无需逐案选择，关闭网页仍继续。正式案件优先，同一异常事件去重。案件资料以 JSON
	文本发送，不传图片。AI
	自动审核低、中风险并结案归档；通过的低风险案件清理证据副本，仅留精简回执。高风险交管理员，证据不足／矛盾单独待核查。AI不执行封禁，不覆盖人工审核。配置由同一组织共用。
</p>
{#if notice}<p role="status" class="mb-3 panel p-3">{notice}</p>{/if}
{#if failure}<p role="alert" class="mb-3 panel p-3 text-red-400">{failure}</p>{/if}
{#if data.canConfigure}
	<section class="mb-5 space-y-4 panel p-5">
		<h3 class="font-semibold">模型接口设置</h3>
		<label class="flex gap-2"
			><input
				type="checkbox"
				bind:checked={autoEnabled}
			/>启用后台自动审核（包含历史未审核案件）</label
		>
		<label class="flex gap-2"
			><input
				type="checkbox"
				bind:checked={autoCloseEnabled}
			/>自动完成低、中风险审核并结案归档</label
		>
		<label class="flex gap-2"
			><input
				type="checkbox"
				bind:checked={deleteLowRisk}
				disabled={!autoCloseEnabled}
			/>低风险通过后清理详细证据副本（保留精简审核回执）</label
		>
		<p class="text-sm text-mist-400">
			0–25且建议通过、无关键缺失：清理；其他0–60：自动结案；65–100：管理员审核。关键证据不足或冲突不会自动通过。已有处罚、举报、人工结论或处置重试记录会保留供核查。原始击杀统计不删除。
		</p>
		<label class="block"
			>每天自动请求上限<input
				class="ml-2 input"
				type="number"
				min="1"
				max="1000"
				bind:value={dailyLimit}
			/></label
		>
		<p class="text-sm text-mist-400">
			今日自动请求：{data.config?.dailyRequests ?? 0} / {data.config?.dailyLimit ??
				100}（UTC日界线）。每组织最多65秒一次；达到上限后保留队列至次日。失败最多尝试3次并退避，不会无限消耗token。
		</p>
		<label class="block"
			>API 基础地址<input
				class="mt-1 input w-full"
				bind:value={baseUrl}
				placeholder="https://服务商域名/v1"
			/></label
		>
		<label class="block"
			>API 密钥（{data.config?.hasKey ? '已保存；留空保留，改地址须重新填写' : '尚未配置'}）<input
				class="mt-1 input w-full"
				type="password"
				autocomplete="new-password"
				bind:value={apiKey}
			/></label
		>
		<label class="block"
			>模型名称<input
				class="mt-1 input w-full"
				list="ai-models"
				bind:value={model}
				placeholder="填写服务商的模型 ID，或读取列表后选择"
			/></label
		>
		<datalist id="ai-models"
			>{#each models as id}<option value={id}></option>{/each}</datalist
		>
		<label class="block"
			>最大输出 token<input
				class="ml-2 input"
				type="number"
				min="256"
				max="4096"
				bind:value={maxTokens}
			/></label
		>
		<label class="block"
			>接口 token 参数<select class="ml-2 input" bind:value={tokenParameter}
				><option value="max_tokens">max_tokens（常用兼容接口）</option><option
					value="max_completion_tokens">max_completion_tokens</option
				></select
			></label
		>
		<p class="text-sm text-mist-400">
			先保存地址和密钥，再读取模型列表；不支持列表的服务商可手动填写。仅支持公网 HTTPS
			接口。测试不发送案件，但可能产生少量费用。同一组织请求间隔 65 秒。
		</p>
		<div class="flex flex-wrap gap-2">
			<button class="btn" disabled={busy} onclick={() => run('save')}>保存配置</button>
			<button class="btn" disabled={busy || !data.config} onclick={() => run('models')}
				>读取模型列表</button
			>
			<button class="btn" disabled={busy || !data.config?.model} onclick={() => run('test')}
				>测试已保存的模型</button
			>
			<button class="btn" disabled={busy || !data.config} onclick={() => run('delete')}
				>删除密钥并停用</button
			>
		</div>
	</section>
{/if}
<section class="mb-5 space-y-4 panel p-5">
	<h3 class="font-semibold">自动初审队列与结果</h3>
	<p class="text-sm text-mist-400">
		自动处理所有未审核案件，包括历史积压；新提示词会重新审核尚未结案的旧建议。达到每日上限后次日继续。
	</p>
	{#each data.cases as c}
		<div class="border-b border-white/10 pb-3">
			<a href={`/server/${page.params.id}/players/${c.steamId}`}
				>{c.name || '未知昵称'} · SteamID 尾号 {c.steamId.slice(-6)}</a
			><small class="block"
				>{new Date(c.createdAt).toLocaleString()} · 案件 {c.id.slice(0, 8)}</small
			><AiCaseSummary
				job={data.jobs.find((j) => j.caseId === c.id)}
				enabled={!!data.config?.autoEnabled && !!data.config?.model}
			/>
		</div>
	{:else}<p>暂无案件。</p>{/each}
	<details>
		<summary>查看当前审核提示词 · {data.promptVersion}</summary>
		<pre class="mt-3 overflow-auto text-sm whitespace-pre-wrap">{data.prompt}</pre>
	</details>
</section>
<details class="panel p-5">
	<summary>手动复核与 JSON 预览（可选，无需逐案操作）</summary>
	<section class="mt-4 space-y-4">
		<h3 class="font-semibold">案件辅助审核</h3>
		<p class="text-sm text-mist-400">
			发送案件完整冻结证据、玩家整局击杀与死亡、案件180秒全服交战背景、举报、在线会话和金钱观测，以及规则快照、统计结果、处置与审核记录。使用可还原的列式JSON压缩，无事件抽样；非致死伤害未采集会明确标记。昵称和举报内容仅作为证据数据。历史案件最多20条，并注明是否截断。超过256
			KB会提示且不发送。
		</p>
		<label class="block"
			>选择最近案件<select
				class="mt-1 input w-full"
				bind:value={caseId}
				onchange={() => {
					preview = '';
					result = '';
					review = null;
				}}
				><option value="">请选择案件</option
				>{#each data.cases.filter((c) => c.status !== 'AI_CLEARED') as c}<option value={c.id}
						>{c.name || '未知昵称'} · SteamID 尾号 {c.steamId.slice(-6)} · {new Date(
							c.createdAt
						).toLocaleString()} · {c.status} · {c.id.slice(0, 8)}</option
					>{/each}</select
			></label
		>
		{#if !data.cases.length}<p>暂无案件。</p>{/if}
		<div class="flex gap-2">
			<button class="btn" disabled={busy || !caseId} onclick={() => run('preview')}
				>预览案件 JSON</button
			><button
				class="btn"
				disabled={busy || !caseId || !data.config?.model}
				onclick={() => run('review')}>{busy ? '处理中…' : '发送至已配置模型并预判'}</button
			>
		</div>
		{#if !data.config}<p>尚未配置模型，请组织管理员在此页面设置。</p>{/if}
		{#if review}<h4 class="font-semibold">{review.verdict} · {review.model}</h4>
			<p>{review.summary}</p>
			<ul>
				{#each review.reasons as reason}<li class="my-2">
						{reason.text}<small class="block text-mist-400">证据：{reason.evidence}</small>
					</li>{/each}
			</ul>
			<h4>数值矛盾</h4>
			{#each review.contradictions as item}<p>{item}</p>{:else}<p>
					模型未列出矛盾；仍须人工核对。
				</p>{/each}
			<h4>缺失证据</h4>
			{#each review.missingEvidence as item}<p>{item}</p>{:else}<p>
					模型未列出缺失项。
				</p>{/each}{/if}
		{#if result}<h4 class="font-semibold">手动预览结果（此按钮不执行自动结案）</h4>
			<pre
				class="max-h-[36rem] overflow-auto text-sm break-all whitespace-pre-wrap">{result}</pre>{/if}
		{#if preview}<details open>
				<summary>实际案件资料 JSON</summary>
				<pre class="max-h-96 overflow-auto text-xs break-all whitespace-pre-wrap">{preview}</pre>
			</details>{/if}
	</section>
</details>
