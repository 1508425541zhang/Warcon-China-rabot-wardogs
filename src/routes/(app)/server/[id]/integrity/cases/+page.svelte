<script lang="ts">
	import IntegrityCaseList from '$lib/components/IntegrityCaseList.svelte';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	let base = $derived(`/server/${data.server.id}/integrity/cases`);
	const href = (page: number) => `${base}?page=${page}&before=${encodeURIComponent(data.before)}`;
	let numbers = $derived(
		Array.from(
			{ length: Math.min(7, data.pages) },
			(_, i) => Math.max(1, Math.min(data.page - 3, data.pages - 6)) + i
		)
	);
</script>

<svelte:head><title>证据案件归档 · Warcon China</title></svelte:head>
<header class="mb-5 space-y-3">
	<a class="text-accent" href={`/server/${data.server.id}/integrity`}>← 返回风控总览</a>
	<h1 class="text-2xl font-semibold">证据案件 · 查看与归档</h1>
	<p class="text-sm text-mist-300">
		所有案件按时间倒序保留，每页20条。归档只收纳历史记录，不改变审核结论或处罚。点击“证据与审核”展开详情。
	</p>
	<div class="flex flex-wrap gap-4">
		<span>共 {data.total} 条 · 第 {data.page} / {data.pages} 页</span><a
			class="text-accent"
			href={base}>刷新到最新案件</a
		>
	</div>
</header>
<IntegrityCaseList {data} />
<nav aria-label="案件分页" class="mb-6 flex flex-wrap items-center justify-center gap-2">
	{#if data.page > 1}<a class="btn" href={href(data.page - 1)}>上一页</a>{/if}
	{#if numbers[0] > 1}<a class="btn" href={href(1)}>1</a>{#if numbers[0] > 2}<span>…</span
			>{/if}{/if}
	{#each numbers as n}<a
			class="btn"
			class:bg-accent={n === data.page}
			aria-current={n === data.page ? 'page' : undefined}
			href={href(n)}>{n}</a
		>{/each}
	{#if numbers[numbers.length - 1] < data.pages}{#if numbers[numbers.length - 1] < data.pages - 1}<span
				>…</span
			>{/if}<a class="btn" href={href(data.pages)}>{data.pages}</a>{/if}
	{#if data.page < data.pages}<a class="btn" href={href(data.page + 1)}>下一页</a>{/if}
</nav>
