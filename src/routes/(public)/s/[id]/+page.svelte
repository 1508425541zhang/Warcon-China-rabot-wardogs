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
	let pct = $derived(
		view.maxPlayers ? Math.min(100, Math.round((view.players / view.maxPlayers) * 100)) : 0
	);
	const colorOf = (faction: string | null) =>
		view.scores.find((f) => f.name === faction)?.colorHex || '#5E5E66';
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

		{#if view.scores.length}
			<div
				class="mt-3 grid gap-2"
				style="grid-template-columns: repeat({view.scores.length}, minmax(0, 1fr))"
			>
				{#each view.scores as f (f.name)}
					<div class="panel px-3 py-3">
						<div class="truncate caps" style="color:{f.colorHex || 'inherit'}">{f.name}</div>
						<div
							class="mt-0.5 font-display text-3xl leading-none font-semibold tabular {f.name ===
							leader
								? 'text-mist-100'
								: 'text-mist-400'}"
						>
							{fmtNum(f.score)}
						</div>
						<div class="mt-2 progress">
							<span
								class="progress-bar"
								style="width:{Math.min(
									100,
									Math.round((f.score / cap) * 100)
								)}%; background:{f.colorHex || 'var(--color-accent)'}"
							></span>
						</div>
					</div>
				{/each}
			</div>
			<div class="mt-1.5 text-[12px] text-mist-600">
				First to {cap}{#if view.matchSeconds !== null}
					· {fmtDuration(view.matchSeconds)} played{/if}
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
			<div class="table-wrap">
				<table>
					<thead>
						<tr><th>Player</th><th>Faction</th><th class="num">K</th><th class="num">D</th></tr>
					</thead>
					<tbody>
						<!-- unkeyed: names are not unique -->
						{#each view.roster as p}
							<tr>
								<td class="max-w-[220px] truncate">{p.name}</td>
								<td>
									{#if p.faction}
										<span class="inline-flex items-center gap-1.5">
											<span
												class="inline-block h-2.5 w-2.5 rounded-full"
												style="background:{colorOf(p.faction)}"
											></span>{p.faction}
										</span>
									{:else}<span class="text-mist-600">unassigned</span>{/if}
								</td>
								<td class="num">{p.kills}</td>
								<td class="num">{p.deaths}</td>
							</tr>
						{:else}
							<tr
								><td colspan="4" class="py-6 text-center text-mist-600">Nobody on right now.</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			</div>
			<div class="mt-3 text-[12px] text-mist-600">
				{#if view.observedAt}Updated {fmtAgo(view.observedAt, now)}.{/if}
			</div>
		{/if}
	</div>
</div>
