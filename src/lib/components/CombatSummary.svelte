<script lang="ts">
	// A player's kill-feed record: the headline tiles, then weapons, most-killed and nemeses.
	// Shared by the dossier's Combat section and the public career page; `hrefFor` decides where
	// a name in the lists goes (dossier or public career). Kills, deaths and K/D are not here:
	// the career tiles carry them from the game's own scoreboard.
	import { causeLabel } from '$lib/causes';
	import { fmtNum } from '$lib/format';
	import type { CombatSummary } from '$lib/types';

	let { combat, hrefFor }: { combat: CombatSummary; hrefFor: (steamId: string) => string } =
		$props();

	let maxCause = $derived(Math.max(1, ...combat.causes.map((c) => c.kills)));
	const metres = (m: number | null) => (m === null ? '—' : `${fmtNum(m)} m`);
	let tiles = $derived<[string, string][]>([
		['Headshots', fmtNum(combat.headshots)],
		['Longest shot', metres(combat.longestM)],
		['Average shot', metres(combat.avgDistanceM)],
		[
			'Team kills',
			combat.teamKilled
				? `${combat.teamKills} · ${combat.teamKilled} taken`
				: String(combat.teamKills)
		],
		['Suicides', String(combat.suicides)]
	]);
</script>

<div class="mb-4 grid grid-cols-2 gap-3 md:grid-cols-4">
	{#each tiles as [label, value] (label)}
		<div class="rounded-ctl border border-black bg-ink-950 px-3.5 py-3">
			<div class="caps text-mist-400">{label}</div>
			<div class="mt-1 font-display text-xl font-semibold tabular">{value}</div>
		</div>
	{/each}
</div>
<div class="grid grid-cols-1 gap-4 md:grid-cols-3">
	<div>
		<span class="field-label">Weapons</span>
		{#each combat.causes as c (c.cause)}
			<div class="mb-2">
				<div class="mb-0.5 flex justify-between text-[13px]">
					<span>{causeLabel(c.cause)}</span><span class="font-mono text-mist-400 tabular"
						>{c.kills}</span
					>
				</div>
				<div class="progress">
					<span class="progress-bar" style="width:{(c.kills / maxCause) * 100}%"></span>
				</div>
			</div>
		{:else}<div class="text-[13px] text-mist-600">No kills yet.</div>{/each}
	</div>
	<div>
		<span class="field-label">Most killed</span>
		{#each combat.victims as v (v.steamId)}
			<div class="flex justify-between text-[13px]">
				<a href={hrefFor(v.steamId)} class="hover:text-accent hover:underline">{v.name}</a>
				<span class="font-mono text-mist-400 tabular">{v.kills}</span>
			</div>
		{:else}<div class="text-[13px] text-mist-600">Nobody yet.</div>{/each}
	</div>
	<div>
		<span class="field-label">Nemeses</span>
		{#each combat.nemeses as n (n.steamId)}
			<div class="flex justify-between text-[13px]">
				<a href={hrefFor(n.steamId)} class="hover:text-accent hover:underline">{n.name}</a>
				<span class="font-mono text-mist-400 tabular">{n.deaths}</span>
			</div>
		{:else}<div class="text-[13px] text-mist-600">Nobody yet.</div>{/each}
	</div>
</div>
