<script lang="ts">
	// One match: the header, final scores, the score over time, awards and the scoreboard from the
	// page's data, and the match's kill feed from the kills route narrowed to it, paged.
	import { api, errorMessage, qs } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import MatchPanel from '$lib/components/MatchPanel.svelte';
	import FactionChip from '$lib/components/FactionChip.svelte';
	import { causeLabel } from '$lib/causes';
	import { fmtDuration } from '$lib/format';
	import type { KillView } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let view = $derived(data.match);
	const PAGE = 100;
	let kills = $state<KillView[]>([]);
	let more = $state(false);
	let loading = $state(false);
	let seq = 0;

	async function load(append = false) {
		const my = ++seq;
		const last = append ? kills[kills.length - 1] : undefined;
		loading = true;
		try {
			const r = await api<{ kills: KillView[] }>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/kills${qs({
					match: view.match.id,
					limit: PAGE,
					before: last?.ts,
					beforeTime: last?.eventTime
				})}`
			);
			if (my !== seq) return;
			kills = last ? [...kills, ...r.kills] : r.kills;
			more = r.kills.length === PAGE;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			if (my === seq) loading = false;
		}
	}
	$effect(() => {
		void view.match.id;
		kills = [];
		if (view.kills) void load();
	});
	const dossier = (steamId: string) =>
		`/server/${encodeURIComponent(id)}/players/${encodeURIComponent(steamId)}`;
	const factions = $derived(
		view.factions.map((f) => ({ name: f.name, colorHex: f.colorHex ?? '', score: 0 }))
	);
	const causeText = (k: KillView) =>
		causeLabel(k.cause) || (k.tags.includes('Falling') ? 'Fall' : '—');
</script>

<div class="panel">
	<a href="/server/{encodeURIComponent(id)}/matches" class="caps text-mist-400 hover:text-mist-100"
		>← Matches</a
	>
	<div class="mt-3">
		<MatchPanel {view} hrefFor={dossier} showIds />
	</div>
	{#if view.kills}
		<span class="mt-4 field-label">Kill feed · {view.kills}</span>
		<div class="table-wrap">
			<table>
				<thead>
					<tr
						><th>Clock</th><th>Killer</th><th>Victim</th><th>Cause</th><th class="num">Distance</th
						><th></th></tr
					>
				</thead>
				<tbody>
					{#each kills as k (k.eventId)}
						<tr class={k.teamKill ? 'text-warn' : ''}>
							<td class="font-mono text-[12px] whitespace-nowrap text-mist-400"
								>{fmtDuration(k.eventTime)}</td
							>
							<td>
								{#if k.killer}
									<a href={dossier(k.killer.steamId)} class="hover:text-accent hover:underline"
										>{k.killer.name}</a
									>
									{#if k.killer.faction}<FactionChip
											faction={k.killer.faction}
											scores={factions}
										/>{/if}
								{:else}<span class="text-mist-600">—</span>{/if}
							</td>
							<td>
								<a href={dossier(k.victim.steamId)} class="hover:text-accent hover:underline"
									>{k.victim.name}</a
								>
								{#if k.victim.faction}<FactionChip
										faction={k.victim.faction}
										scores={factions}
									/>{/if}
							</td>
							<td class="text-mist-200">{causeText(k)}</td>
							<td class="num">{k.distanceM === null ? '—' : `${Math.round(k.distanceM)} m`}</td>
							<td class="whitespace-nowrap">
								{#if k.teamKill}<span class="chip">team kill</span>{/if}
								{#if k.suicide}<span class="chip">suicide</span>{/if}
								{#if k.headshot}<span class="chip">headshot</span>{/if}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-4 text-center text-mist-600"
								>{loading ? 'Loading…' : 'No kills of this match were received.'}</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		{#if more}
			<button class="mt-3 btn" onclick={() => load(true)} disabled={loading}>
				{loading ? 'Loading…' : 'Load older kills'}
			</button>
		{/if}
	{/if}
	<p class="note">
		Kills, deaths and cash are the game's own scoreboard counters over this match; time is how long
		each player was on during it. {#if view.hasFeed}Headshots, team kills, vehicle kills and streaks
			come from the kill feed, newest kill first below.{:else}This server has no kill feed, so there
			are no feed columns.{/if} Awards need a match of twenty minutes; best K/D needs ten kills.
	</p>
</div>
