<script lang="ts">
	// A server's matches as the panel's Matches tab lists them: when each started, the map, how
	// long it ran, who won with the final scores, and how many played. A match that has ended
	// links to its page; the one in progress shows the scores as they stand and does not (the
	// Overview has it).
	import { factionColor, fmtTime, mapName } from '$lib/format';
	import {
		durationOf,
		fmtLength,
		resultLine,
		type LiveFaction,
		type MatchSummary
	} from '$lib/matches';

	let {
		matches,
		live = [],
		hrefFor,
		loading = false
	}: {
		matches: MatchSummary[];
		live?: LiveFaction[];
		hrefFor: (m: MatchSummary) => string;
		loading?: boolean;
	} = $props();
	let now = $state(Date.now());
	$effect(() => {
		const t = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(t);
	});
	const colorOf = (name: string | null) =>
		live.find((f) => f.name === name)?.colorHex || factionColor(name);
	let liveScores = $derived(
		[...live]
			.sort((a, b) => b.score - a.score)
			.map((f) => `${f.name} ${f.score}`)
			.join(' · ')
	);
</script>

<div class="table-wrap">
	<table>
		<thead>
			<tr
				><th>Started</th><th>Map</th><th class="num">Length</th><th>Result</th><th class="num"
					>Players</th
				></tr
			>
		</thead>
		<tbody>
			{#each matches as m (m.id)}
				{@const r = resultLine(m)}
				<tr>
					<td class="whitespace-nowrap">
						{#if m.endedAt}<a href={hrefFor(m)} class="hover:text-accent hover:underline"
								>{fmtTime(m.startedAt)}</a
							>{:else}{fmtTime(m.startedAt)}{/if}
					</td>
					<td>
						{#if m.endedAt}<a href={hrefFor(m)} class="hover:text-accent hover:underline"
								>{mapName(m.map)}</a
							>{:else}{mapName(m.map)}{/if}
					</td>
					<td class="num whitespace-nowrap">{fmtLength(durationOf(m, now))}</td>
					<td>
						{#if r.kind === 'running'}
							<span class="text-mist-400">In progress</span>
							{#if liveScores}<span class="ml-2 text-[12.5px] whitespace-nowrap text-mist-600"
									>{liveScores}</span
								>{/if}
						{:else if r.kind === 'abandoned'}<span class="text-mist-600">Abandoned</span>
						{:else}
							{#if m.winner}<span class="font-semibold" style="color:{colorOf(m.winner)}"
									>{m.winner}</span
								>{:else}<span class="text-mist-400">Draw</span>{/if}
							<span class="ml-2 text-[12.5px] whitespace-nowrap text-mist-400">{r.scores}</span>
						{/if}
					</td>
					<td class="num">{m.endedAt ? m.players || m.peakPlayers : m.peakPlayers}</td>
				</tr>
			{:else}
				<tr
					><td colspan="5" class="py-6 text-center text-mist-600"
						>{loading ? 'Loading…' : 'No matches recorded yet.'}</td
					></tr
				>
			{/each}
		</tbody>
	</table>
</div>
