<script lang="ts">
	// The public match history, one page of cards at a time; the pager is plain links, so any
	// page can be shared.
	import MatchCards from '$lib/components/MatchCards.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let base = $derived(`/s/${encodeURIComponent(data.heading.id)}`);
	let list = $derived(data.list);
	const pageHref = (p: number) => (p > 1 ? `${base}/matches?page=${p}` : `${base}/matches`);
</script>

<svelte:head>
	<title>对局 · {data.heading.name} · {data.appName}</title>
	<meta name="description" content="Match history of {data.heading.name}." />
</svelte:head>

<div class="rise">
	<MatchCards matches={list.matches} live={list.live} hrefFor={(m) => `${base}/matches/${m.id}`} />
	{#if list.pages > 1}
		<div class="mt-4 flex items-center gap-2 text-[12.5px] text-mist-400">
			{#if list.page > 1}<a href={pageHref(list.page - 1)} class="btn btn-sm">← 较新</a>{:else}<span
					class="pointer-events-none btn btn-sm opacity-50">← 较新</span
				>{/if}
			<span>第 {list.page} of {list.pages}</span>
			{#if list.page < list.pages}<a href={pageHref(list.page + 1)} class="btn btn-sm">更早 →</a
				>{:else}<span class="pointer-events-none btn btn-sm opacity-50">更早 →</span>{/if}
		</div>
	{/if}
	<p class="note">对局结束后可打开计分板；正在进行的对局显示在实时页面。</p>
</div>
