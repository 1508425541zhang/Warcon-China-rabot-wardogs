<script lang="ts">
	import type { aiJobViews } from '$lib/server/integrity/ai-queue';
	let {
		job,
		enabled = false
	}: { job?: Awaited<ReturnType<typeof aiJobViews>>[number]; enabled?: boolean } = $props();
	const dispositions: Record<string, string> = {
		AI_CLEARED: '低风险：AI已审核并清理详细证据副本',
		AI_ARCHIVED: 'AI已完成审核并结案归档，无需逐案点击',
		ADMIN_REVIEW: '转交管理员：高风险或存在关联处置／举报',
		NEEDS_DATA: '旧版待核查：等待重新分流',
		AI_ARCHIVED_UNRESOLVED: 'AI已按证据不足结案归档，完整保留资料供追查；不表示确认正常',
		SKIPPED_REVIEWED: '案件已由其他审核流程处理，AI未覆盖结论',
		ADVISORY: '仅建议：自动结案开关未启用'
	};
	const states: Record<string, string> = {
		pending: '等待自动初审',
		running: 'AI正在初审',
		done: 'AI已初审',
		error: 'AI初审失败',
		skipped: '已跳过'
	};
</script>

<div class="my-2 rounded border border-white/10 p-3 text-sm">
	<strong
		>{job
			? (states[job.state] ?? job.state)
			: enabled
				? '等待后台发现未审核案件'
				: 'AI自动初审未启用／未配置'}</strong
	>
	{#if job?.disposition}<p class="mt-1 font-semibold">
			{dispositions[job.disposition] ?? job.disposition}
		</p>{/if}
	{#if job?.result}
		<div class="mt-1 text-accent">
			可疑度：{job.result.suspicionPercent === null
				? '无法估计'
				: `${job.result.suspicionPercent}%`} · 证据质量：{job.result.evidenceQuality}
		</div>
		<p class="mt-1">{job.result.verdict}：{job.result.summary}</p>
		<p class="mt-1 text-xs text-mist-400">
			AI主观估计，不是作弊概率；自动结案不产生封禁，不覆盖人工结论。
		</p>
		<details class="mt-2">
			<summary class="cursor-pointer">理由、正常解释与数字核对</summary>
			{#each job.result.reasons as reason}<p class="mt-2">
					{reason.text}<small class="block text-mist-400">依据：{reason.evidence}</small>
				</p>{/each}
			<h4 class="mt-3 font-semibold">可能的正常解释</h4>
			{#each job.result.alternatives as item}<p>{item}</p>{:else}<p>未列出。</p>{/each}
			<h4 class="mt-3 font-semibold">数字／口径矛盾</h4>
			{#each job.result.contradictions as item}<p>{item}</p>{:else}<p>模型未列出矛盾。</p>{/each}
			<h4 class="mt-3 font-semibold">待核查信息</h4>
			{#each job.result.missingEvidence as item}<p>{item}</p>{:else}<p>未列出。</p>{/each}
		</details>
	{:else if job?.lastError}<p class="mt-1 text-warn">
			{job.lastError}（尝试 {job.attempts}/3）
		</p>{/if}
</div>
