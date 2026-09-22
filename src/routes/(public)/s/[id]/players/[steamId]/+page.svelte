<script lang="ts">
	// A public career: the same panel the dossier shows, with the kill-feed record between the
	// rank tiles and the tables, under the player's name and the cached Steam avatar (never
	// fetched for a public viewer).
	import CareerPanel from '$lib/components/CareerPanel.svelte';
	import CombatSummary from '$lib/components/CombatSummary.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let base = $derived(`/s/${encodeURIComponent(data.heading.id)}`);
</script>

<svelte:head>
	<title>{data.player.name} · {data.heading.name} · {data.appName}</title>
	<meta name="description" content="{data.player.name}'s career on {data.heading.name}." />
</svelte:head>

<div class="rise">
	<div class="mb-4 flex flex-wrap items-center gap-3">
		<a href="{base}/leaderboard" class="caps text-mist-400 hover:text-mist-100">← Leaderboard</a>
		<h2 class="flex items-center gap-2 text-xl font-semibold tracking-tight">
			{#if data.player.avatar}<img
					src={data.player.avatar}
					alt=""
					class="h-8 w-8 rounded-[2px] border border-black"
					referrerpolicy="no-referrer"
				/>{/if}
			<span class="truncate">{data.player.name}</span>
		</h2>
	</div>
	<div class="panel">
		<CareerPanel
			career={data.career}
			serverName={data.heading.name}
			orgName={data.heading.orgName}
			multiServer={data.multiServer}
			matchHref={(m) => `/s/${encodeURIComponent(m.serverId)}/matches/${m.matchId}`}
		>
			{#if data.combat}
				<span class="field-label">Combat</span>
				<CombatSummary combat={data.combat} hrefFor={(steamId) => `${base}/players/${steamId}`} />
				<div class="mb-4"></div>
			{/if}
		</CareerPanel>
	</div>
</div>
