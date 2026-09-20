<script lang="ts">
	// A player's career: rank, streak, results overall, by map and by faction, and the last ten
	// matches. Shared by the dossier's Career section and the public career page.
	import type { Snippet } from 'svelte';
	import { fmtNum, fmtTime, mapName } from '$lib/format';
	import { kdRatio, type CareerView } from '$lib/leaderboard';

	let {
		career,
		serverName,
		orgName,
		/** more than one server in the org: name the server on each match */
		multiServer = false,
		/** rendered between the rank tiles and the tables (the public career puts combat here) */
		children
	}: {
		career: CareerView;
		serverName: string;
		orgName: string;
		multiServer?: boolean;
		children?: Snippet;
	} = $props();

	const kd = (k: number, d: number) => {
		const v = kdRatio(k, d);
		return v === null ? '—' : v.toFixed(2);
	};
	const rank = (n: number | null) => (n === null ? '—' : `#${fmtNum(n)}`);
	const RESULT_TONE = { win: 'text-ok', loss: 'text-danger', draw: 'text-mist-400' } as const;
	const RESULT_LABEL = { win: 'Win', loss: 'Loss', draw: 'Draw' } as const;
</script>

<div class="mb-4 grid grid-cols-2 gap-3 md:grid-cols-4">
	{#each [['Rank here', rank(career.rank.server)], [`Rank, all ${orgName} servers`, rank(career.rank.org)], ['Streak', career.streak ? `${career.streak.n} ${career.streak.kind === 'win' ? 'win' : 'loss'}${career.streak.n === 1 ? '' : career.streak.kind === 'win' ? 's' : 'es'}` : '—'], ['Matches', `${fmtNum(career.matches)} · ${career.wins}-${career.losses}-${career.draws}`]] as [label, value] (label)}
		<div class="rounded-ctl border border-black bg-ink-950 px-3.5 py-3">
			<div class="caps text-mist-400">{label}</div>
			<div class="mt-1 font-display text-xl font-semibold tabular">{value}</div>
		</div>
	{/each}
</div>
<p class="mb-3 text-[12.5px] text-mist-600">
	Rank is the place among every player seen on the servers, by all-time kills, with at least
	{career.rank.floorMinutes} minutes played; a dash means under the floor. Matches are wins-losses-draws.
</p>
{#if children}{@render children()}{/if}
<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
	<div>
		<span class="field-label">By map</span>
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>Map</th><th class="num">Matches</th><th class="num">W</th><th class="num">K/D</th
						></tr
					></thead
				>
				<tbody>
					{#each career.maps as m (m.key)}
						<tr>
							<td>{mapName(m.key)}</td><td class="num">{m.matches}</td><td class="num">{m.wins}</td>
							<td class="num">{kd(m.kills, m.deaths)}</td>
						</tr>
					{:else}
						<tr><td colspan="4" class="py-4 text-center text-mist-600">No matches yet.</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
	<div>
		<span class="field-label">By faction</span>
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>Faction</th><th class="num">Matches</th><th class="num">W</th><th class="num"
							>K/D</th
						></tr
					></thead
				>
				<tbody>
					{#each career.factions as f (f.key)}
						<tr>
							<td>{f.key}</td><td class="num">{f.matches}</td><td class="num">{f.wins}</td>
							<td class="num">{kd(f.kills, f.deaths)}</td>
						</tr>
					{:else}
						<tr><td colspan="4" class="py-4 text-center text-mist-600">No matches yet.</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
</div>
{#if career.last.length}
	<span class="mt-4 field-label"
		>Last {career.last.length === 1 ? 'match' : `${career.last.length} matches`}</span
	>
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<th>Started</th>{#if multiServer}<th>Server</th>{/if}<th>Map</th><th>Faction</th><th
						>Result</th
					>
					<th class="num">K</th><th class="num">D</th>
				</tr>
			</thead>
			<tbody>
				{#each career.last as m (m.matchId)}
					<tr>
						<td class="whitespace-nowrap">{fmtTime(m.startedAt)}</td>
						{#if multiServer}<td>{m.serverName}</td>{/if}
						<td>{m.map ? mapName(m.map) : '—'}</td>
						<td>{m.faction || '—'}</td>
						<td class={m.result ? RESULT_TONE[m.result] : 'text-mist-600'}
							>{m.result ? RESULT_LABEL[m.result] : m.endedAt ? '—' : 'In progress'}</td
						>
						<td class="num">{m.kills}</td><td class="num">{m.deaths}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="note">
		A match counts when a session on {serverName === orgName ? 'the server' : 'a server'} overlapped it;
		kills and deaths in it are from the kill feed.
	</p>
{/if}
