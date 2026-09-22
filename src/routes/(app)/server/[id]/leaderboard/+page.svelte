<script lang="ts">
	// The Leaderboards tab: one board over this server or the organisation, read once per
	// change of the controls (no polling: it is history, not a live view). The URL follows the
	// query so a board can be linked.
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import { api, errorMessage, qs } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import LeaderboardTable from '$lib/components/LeaderboardTable.svelte';
	import {
		boardQueryParams,
		parseBoardQuery,
		type BoardQuery,
		type BoardView
	} from '$lib/leaderboard';
	import { effectiveFeatures } from '$lib/features';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let query = $state<BoardQuery>(parseBoardQuery(page.url.searchParams));
	let board = $state<BoardView | null>(null);
	let loading = $state(false);
	let seq = 0;
	let publicOn = $derived(effectiveFeatures(data.server, data.server).leaderboards);

	async function load(q: BoardQuery) {
		const my = ++seq;
		loading = true;
		try {
			const r = await api<BoardView>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/leaderboard${qs(boardQueryParams(q))}`
			);
			if (my !== seq) return;
			board = r;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			if (my === seq) loading = false;
		}
	}
	$effect(() => {
		const q = query;
		const url = new URL(page.url);
		url.search = qs(boardQueryParams(q));
		if (url.search !== page.url.search) replaceState(url, {});
		void load(q);
	});
</script>

<div class="panel">
	<LeaderboardTable
		{board}
		{query}
		{loading}
		onchange={(q) => (query = q)}
		hrefFor={(steamId) => `/server/${encodeURIComponent(id)}/players/${steamId}`}
		orgName={data.server.orgName}
		showIds
	/>
	<p class="note">
		Kills and deaths are the game's own scoreboard counters, recorded per match; headshots, team
		kills and streaks come from the kill feed; playtime from player sessions, cash as last seen. A
		match counts once it has ended, with the result read against the side the player played. Kills
		per hour leaves seed time out. Names link to the dossier.
		{#if publicOn}This board is also public at <a
				href="/s/{encodeURIComponent(id)}/leaderboard"
				class="text-accent hover:underline">/s/{id}/leaderboard</a
			>.{/if}
	</p>
</div>
