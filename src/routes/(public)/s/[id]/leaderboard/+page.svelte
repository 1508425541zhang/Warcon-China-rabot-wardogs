<script lang="ts">
	// The public leaderboard: the same board as the panel's tab, rendered from the page load and
	// reloaded through the URL whenever a control changes (each load is one rate-limited read).
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { qs } from '$lib/api';
	import LeaderboardTable from '$lib/components/LeaderboardTable.svelte';
	import { boardQueryParams, type BoardQuery } from '$lib/leaderboard';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let loading = $state(false);
	let base = $derived(`/s/${encodeURIComponent(data.heading.id)}`);
	async function change(q: BoardQuery) {
		loading = true;
		try {
			await goto(`${page.url.pathname}${qs(boardQueryParams(q))}`, {
				noScroll: true,
				keepFocus: true
			});
		} finally {
			loading = false;
		}
	}
</script>

<svelte:head>
	<title>Leaderboard · {data.heading.name} · {data.appName}</title>
	<meta
		name="description"
		content="Leaderboard of {data.heading.name}, run by {data.heading.orgName}."
	/>
</svelte:head>

<div class="rise panel">
	<LeaderboardTable
		board={data.board}
		query={data.board.query}
		{loading}
		onchange={change}
		hrefFor={(steamId) => `${base}/players/${steamId}`}
		orgName={data.heading.orgName}
		orgScope={data.orgScope}
		relative
	/>
	<p class="note">
		Kills and deaths from the game's scoreboard, match by match; headshots, team kills and streaks
		from the kill feed; playtime from time seen on the server. A match counts once it has ended.
		Names open a player's career.
	</p>
</div>
