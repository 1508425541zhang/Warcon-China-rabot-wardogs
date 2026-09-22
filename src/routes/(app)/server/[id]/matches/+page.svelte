<script lang="ts">
	// The Matches tab: the server's match history, newest first, one page at a time (no polling:
	// it is history; the match in progress is on the Overview). The address follows the page so
	// it can be linked. A match that has ended opens its page.
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import { api, errorMessage, qs } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import MatchList from '$lib/components/MatchList.svelte';
	import { parsePage, type MatchListView } from '$lib/matches';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let current = $state(parsePage(page.url.searchParams.get('page')));
	let view = $state<MatchListView | null>(null);
	let loading = $state(false);
	let seq = 0;

	async function load(p: number) {
		const my = ++seq;
		loading = true;
		try {
			const r = await api<MatchListView>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/matches${qs({ page: p > 1 ? p : undefined })}`
			);
			if (my !== seq) return;
			view = r;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			if (my === seq) loading = false;
		}
	}
	$effect(() => {
		const p = current;
		const url = new URL(page.url);
		url.search = qs({ page: p > 1 ? p : undefined });
		if (url.search !== page.url.search) replaceState(url, {});
		void load(p);
	});
</script>

<div class="panel">
	<div class="mb-3 flex items-center gap-2 text-[12.5px] text-mist-400">
		<span class="text-mist-200">
			{#if view}{view.total} match{view.total === 1 ? '' : 'es'}{#if loading}
					· loading…{/if}{:else}Loading…{/if}
		</span>
	</div>
	<MatchList
		matches={view?.matches ?? []}
		live={view?.live ?? []}
		{loading}
		hrefFor={(m) => `/server/${encodeURIComponent(id)}/matches/${m.id}`}
	/>
	{#if view && view.pages > 1}
		<div class="mt-3 flex items-center gap-2 text-[12.5px] text-mist-400">
			<button class="btn btn-sm" disabled={current <= 1 || loading} onclick={() => (current -= 1)}
				>← Newer</button
			>
			<span>Page {current} of {view.pages}</span>
			<button
				class="btn btn-sm"
				disabled={current >= view.pages || loading}
				onclick={() => (current += 1)}>Older →</button
			>
		</div>
	{/if}
	<p class="note">
		Every match the worker saw, kept for good. A match opens to its scoreboard once it has ended;
		the one in progress is on the Overview. Players is everyone who was on during the match.
	</p>
</div>
