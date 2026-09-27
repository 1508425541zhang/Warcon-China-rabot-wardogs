<script lang="ts">
	import IntegrityCaseList from '$lib/components/IntegrityCaseList.svelte';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	let base = $derived(`/server/${data.server.id}/integrity/cases`);
	const href = (page: number) =>
		`${base}?view=${data.view}&page=${page}&before=${encodeURIComponent(data.before)}`;
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
		默认展示待审核案件，每页20条。AI自动结案进入归档；低风险清理后仅保留精简审核回执与去重标识，详细证据副本已删除。
	</p>
	<nav class="flex flex-wrap gap-2" aria-label="案件分类">
		{#each [['pending', '待审核'], ['archived', '已结案／归档'], ['all', '全部保留案件'], ['cleared', '低风险清理回执']] as item}<a
				class="btn"
				aria-current={data.view === item[0] ? 'page' : undefined}
				href={`${base}?view=${item[0]}`}>{item[1]}</a
			>{/each}
	</nav>
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
