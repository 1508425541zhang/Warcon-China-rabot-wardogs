<script lang="ts">
	import type { aiJobViews } from '$lib/server/integrity/ai-queue';
	let {
		job,
		enabled = false
	}: { job?: Awaited<ReturnType<typeof aiJobViews>>[number]; enabled?: boolean } = $props();
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
				? '未进入自动队列（仅近7天未审核案件）'
				: 'AI自动初审未启用／未配置'}</strong
	>
	{#if job?.result}
		<div class="mt-1 text-accent">
			可疑度：{job.result.suspicionPercent === null
				? '无法估计'
				: `${job.result.suspicionPercent}%`} · 证据质量：{job.result.evidenceQuality}
		</div>
		<p class="mt-1">{job.result.verdict}：{job.result.summary}</p>
		<p class="mt-1 text-xs text-mist-400">
			AI主观估计，不是作弊概率；不执行处罚，人工审核状态独立。
		</p>
		<details class="mt-2">
			<summary class="cursor-pointer">理由、正常解释与数字核对</summary>
			{#each job.result.reasons as reason}<p class="mt-2">
					{reason.text}<small class="block text-mist-400">依据：{reason.evidence}</small>
				</p>{/each}
			<h4 class="mt-3 font-semibold">可能的正常解释</h4>
			{#each job.result.alternatives as item}<p>{item}</p>{:else}<p>未列出。</p>{/each}
			<h4 class="mt-3 font-semibold">数字／口径矛盾</h4>
			{#each job.result.contradictions as item}<p>{item}</p>{:else}<p>
					模型未列出，仍需人工核对。
				</p>{/each}
			<h4 class="mt-3 font-semibold">待核查信息</h4>
			{#each job.result.missingEvidence as item}<p>{item}</p>{:else}<p>未列出。</p>{/each}
		</details>
	{:else if job?.lastError}<p class="mt-1 text-warn">
			{job.lastError}（尝试 {job.attempts}/3）
		</p>{/if}
</div>
