<script lang="ts">
	// The public live page: map, mode, scores, player count, join code and who is on, polled from
	// the public JSON every twenty seconds while the tab is visible. One stack on a phone; from
	// the desktop breakpoint the roster sits beside the map and scores so it all fits one screen.
	import { api } from '$lib/api';
	import { poll } from '$lib/poll';
	import { fmtAgo, fmtDuration, fmtNum, mapName, prettify, expSetLabel } from '$lib/format';
	import { scoreCapOf } from '$lib/match';
	import { toast } from '$lib/toast.svelte';
	import MapArt from '$lib/components/MapArt.svelte';
	import Pulse from '$lib/components/Pulse.svelte';
	import type { PublicStatus } from '$lib/server/public';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let pushed = $state<PublicStatus | null>(null);
	let view = $derived(pushed && pushed.serverId === data.view.serverId ? pushed : data.view);

	$effect(() => {
		const id = data.view.serverId;
		return poll(async () => {
			try {
				const r = await api<{ server: PublicStatus }>(
					'GET',
					`/api/public/servers/${encodeURIComponent(id)}`
				);
				pushed = r.server;
			} catch {
				/* the page keeps what it has; the next poll tries again */
			}
		}, 20_000);
	});
	let now = $state(Date.now());
	$effect(() => {
		const t = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(t);
	});
	const NO_CATALOG = { maps: [], lightings: [], experiences: [] };
	let mode = $derived(
		[
			view.experiences.length ? expSetLabel(NO_CATALOG, view.experiences) : '',
			view.lighting ? prettify(view.lighting) : ''
		]
			.filter(Boolean)
			.join(' · ')
	);
	let cap = $derived(scoreCapOf({ scoreCap: view.scoreCap }));
	let leader = $derived(
		view.scores.length ? [...view.scores].sort((a, b) => b.score - a.score)[0].name : null
	);
	type Team = {
		name: string;
		colorHex: string | null;
		score: number | null;
		players: PublicStatus['roster'];
	};
	// One column per faction in scoreboard order, its players under its score; factions only the
	// roster names (no score yet) follow, and players without a faction come last.
	let teams = $derived.by((): Team[] => {
		const out: Team[] = view.scores.map((f) => ({ ...f, players: [] }));
		const unassigned: Team = { name: 'No team yet', colorHex: null, score: null, players: [] };
		for (const p of view.roster) {
			let t = p.faction ? out.find((x) => x.name === p.faction) : unassigned;
			if (!t) {
				t = { name: p.faction!, colorHex: null, score: null, players: [] };
				out.push(t);
			}
			t.players.push(p);
		}
		if (unassigned.players.length) out.push(unassigned);
		return out;
	});
	let pct = $derived(
		view.maxPlayers ? Math.min(100, Math.round((view.players / view.maxPlayers) * 100)) : 0
	);
	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast('Join code copied.', 'ok');
		} catch {
			window.prompt('Copy the join code:', text);
		}
	}
</script>

<svelte:head>
	<title>{view.name} · {data.appName}</title>
	<meta
		name="description"
		content="{view.name}: {view.ok
			? `${view.players} online on ${mapName(view.map)}`
			: 'could not be reached'}"
	/>
</svelte:head>

<div class="grid rise grid-cols-1 gap-3 lg:grid-cols-2 lg:items-start lg:gap-4">
	<div>
		<div class="relative overflow-hidden rounded-card border border-black bg-ink-900">
			{#if view.map}
				<MapArt
					map={view.map}
					lighting={view.lighting}
					variant="720"
					alt=""
					class="rounded-none! border-0!"
				/>
			{:else}
				<div class="map-art map-art-720"></div>
			{/if}
			<div
				class="absolute inset-x-0 bottom-0 flex items-end justify-between gap-3 bg-ink-950/85 px-4 py-2.5"
			>
				<div class="min-w-0">
					<div
						class="font-display text-[22px] leading-tight font-semibold tracking-[0.06em] uppercase"
					>
						{mapName(view.map)}
					</div>
					<div class="caps text-mist-400">{mode || 'Waiting for the first look'}</div>
				</div>
				<span class="inline-flex shrink-0 items-center gap-1.5 caps text-mist-400">
					<Pulse ok={view.observedAt ? view.ok : undefined} />
					{view.ok ? 'live' : view.observedAt ? 'offline' : 'checking'}
				</span>
			</div>
		</div>

		{#if !view.ok}
			<div class="mt-3 callout mb-0 border-l-danger">
				<b class="text-danger"
					>{view.observedAt ? 'The server could not be reached' : 'Not looked at yet'}</b
				>
				{#if view.observedAt}<span class="text-mist-400">
						· last checked {fmtAgo(view.observedAt, now)}</span
					>{/if}
			</div>
		{/if}

		<div class="mt-3 panel py-4">
			<div class="flex items-baseline justify-between gap-3">
				<span class="caps text-mist-400">Players</span>
				<span class="font-display text-3xl leading-none font-semibold tabular"
					>{fmtNum(view.players)}<span class="text-xl text-mist-400">
						/ {view.maxPlayers === null ? '—' : fmtNum(view.maxPlayers)}</span
					></span
				>
			</div>
			<div class="mt-2 progress"><span class="progress-bar" style="width:{pct}%"></span></div>
			{#if view.reservedSlots}
				<div class="note">
					+ {view.reservedSlots} slot{view.reservedSlots === 1 ? '' : 's'} held for reserved players.
				</div>
			{/if}
		</div>

		{#if view.joinCode}
			<div class="mt-3 panel py-4">
				<span class="label-sm">Join code</span>
				<div class="join join-wrap w-full">
					<code class="flex input items-center font-mono text-[13px] break-all select-all"
						>{view.joinCode}</code
					>
					<button type="button" class="btn" onclick={() => copy(view.joinCode!)}>Copy</button>
				</div>
				<p class="note">Paste it into the game's server browser to join.</p>
			</div>
		{/if}
	</div>

	<div>
		{#if view.ok}
			{#if teams.length}
				<div
					class="grid grid-cols-1 gap-3 sm:grid-cols-[repeat(var(--teams),minmax(0,1fr))]"
					style="--teams:{teams.length}"
				>
					{#each teams as t (t.name)}
						<div class="table-wrap">
							<div class="border-b border-black bg-ink-900 px-3 py-3">
								<div class="flex items-baseline justify-between gap-2">
									<span class="truncate caps" style="color:{t.colorHex || 'inherit'}">{t.name}</span
									>
									{#if t.score !== null}
										<span
											class="font-display text-2xl leading-none font-semibold tabular {t.name ===
											leader
												? 'text-mist-100'
												: 'text-mist-400'}">{fmtNum(t.score)}</span
										>
									{/if}
								</div>
								{#if t.score !== null}
									<div class="mt-2 progress">
										<span
											class="progress-bar"
											style="width:{Math.min(
												100,
												Math.round((t.score / cap) * 100)
											)}%; background:{t.colorHex || 'var(--color-accent)'}"
										></span>
									</div>
								{/if}
							</div>
							<table class="team">
								<thead>
									<tr><th>Player</th><th class="num">K</th><th class="num">D</th></tr>
								</thead>
								<tbody>
									<!-- unkeyed: names are not unique -->
									{#each t.players as p}
										<tr>
											<td class="max-w-[220px] truncate">{p.name}</td>
											<td class="num">{p.kills}</td>
											<td class="num">{p.deaths}</td>
										</tr>
									{:else}
										<tr><td colspan="3" class="py-4 text-center text-mist-600">Nobody yet.</td></tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/each}
				</div>
				{#if view.scores.length}
					<div class="mt-1.5 text-[12px] text-mist-600">
						First to {cap}{#if view.matchSeconds !== null}
							· {fmtDuration(view.matchSeconds)} played{/if}
					</div>
				{/if}
			{:else}
				<div class="table-wrap py-6 text-center text-mist-600">Nobody on right now.</div>
			{/if}
			<div class="mt-3 text-[12px] text-mist-600">
				{#if view.observedAt}Updated {fmtAgo(view.observedAt, now)}.{/if}
			</div>
		{/if}
	</div>
</div>

<style>
	/* Three team tables side by side leave little room: tighter cells than the console's tables. */
	table.team :is(th, td) {
		padding-inline: 12px;
	}
	table.team td {
		padding-block: 6px;
	}
</style>
