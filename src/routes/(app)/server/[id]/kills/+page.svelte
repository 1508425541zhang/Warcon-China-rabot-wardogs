<script lang="ts">
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import { api, qs, errorMessage } from '$lib/api';
	import { watchLive, type KillsNotice } from '$lib/live';
	import { causeLabel, knownCauses } from '$lib/causes';
	import { fmtNum, fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import FactionChip from '$lib/components/FactionChip.svelte';
	import {
		EMPTY_FILTER,
		KINDS,
		isEmptyFilter,
		killFilterParams,
		killMatches,
		parseKillFilter,
		type KillFilter
	} from '$lib/kills';
	import type { KillView, LiveView, Status } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	const PAGE = 100;

	// The filter starts from the URL, so a link to "this player's kills here" opens as one, and
	// the URL follows the filter so the view can be shared.
	let filter = $state<KillFilter>(parseKillFilter(page.url.searchParams));
	let kills = $state<KillView[]>([]);
	/** how many match over the whole history; null until the first answer */
	let total = $state<number | null>(null);
	let configured = $state<boolean | null>(null);
	let loading = $state(false);
	/** the last page was full, so there may be older rows */
	let more = $state(false);
	let status = $state<Status | null>(null);
	let seq = 0;

	async function load(append = false) {
		const my = ++seq;
		const last = append ? kills[kills.length - 1] : undefined;
		loading = true;
		try {
			const r = await api<{ configured: boolean; kills: KillView[]; total: number | null }>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/kills${qs({
					...killFilterParams(filter),
					limit: PAGE,
					before: last?.ts,
					beforeTime: last?.eventTime,
					count: append ? '' : '1'
				})}`
			);
			if (my !== seq) return;
			configured = r.configured;
			if (last) {
				const known = new Set(kills.map((k) => k.eventId));
				kills = [...kills, ...r.kills.filter((k) => !known.has(k.eventId))];
			} else {
				kills = r.kills;
				total = r.total;
			}
			more = r.kills.length === PAGE;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			if (my === seq) loading = false;
		}
	}

	// Typing settles before a request goes out; the first load and a select change go at once.
	let first = true;
	$effect(() => {
		const params = killFilterParams(filter);
		const now = first;
		first = false;
		const t = setTimeout(
			() => {
				const url = new URL(page.url);
				url.search = qs(params);
				if (url.search !== page.url.search) replaceState(url, {});
				void load();
			},
			now ? 0 : 250
		);
		return () => clearTimeout(t);
	});

	/** A kill the game just posted joins the top when it is one the filter asks for. */
	function onKills(n: KillsNotice) {
		if (n.serverId !== id || !n.kills.length) return;
		const known = new Set(kills.map((k) => k.eventId));
		const fresh = n.kills.filter((k) => !known.has(k.eventId) && killMatches(filter, k)).reverse();
		if (!fresh.length) return;
		kills = [...fresh, ...kills];
		if (total !== null) total += fresh.length;
	}
	function onLive(v: LiveView) {
		if (v.status) status = v.status;
	}
	$effect(() => {
		void id;
		return watchLive([id], onLive, undefined, onKills);
	});

	/** The named causes plus any the loaded rows carry, so an unnamed weapon is still a choice. */
	let causeOptions = $derived.by(() => {
		const seen = new Map(knownCauses().map((c) => [c.cause, c.label]));
		for (const k of kills)
			if (k.cause && !seen.has(k.cause)) seen.set(k.cause, causeLabel(k.cause));
		if (filter.cause && !seen.has(filter.cause)) seen.set(filter.cause, causeLabel(filter.cause));
		return [...seen]
			.map(([cause, label]) => ({ cause, label }))
			.sort((a, b) => a.label.localeCompare(b.label));
	});
	let filtered = $derived(!isEmptyFilter(filter));
	const clear = () => (filter = { ...EMPTY_FILTER });
	const dossier = (steamId: string) =>
		`/server/${encodeURIComponent(id)}/players/${encodeURIComponent(steamId)}`;
	const causeText = (k: KillView) =>
		causeLabel(k.cause) || (k.tags.includes('Falling') ? 'Fall' : '—');
</script>

<div class="panel">
	<div class="mb-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
		<input
			class="input"
			type="search"
			placeholder="Any side: name or SteamID"
			aria-label="Filter by player on either side"
			bind:value={filter.player}
		/>
		<input
			class="input"
			type="search"
			placeholder="Killer: name or SteamID"
			aria-label="Filter by killer"
			bind:value={filter.killer}
		/>
		<input
			class="input"
			type="search"
			placeholder="Victim: name or SteamID"
			aria-label="Filter by victim"
			bind:value={filter.victim}
		/>
		<select class="input" aria-label="Filter by weapon or vehicle" bind:value={filter.cause}>
			<option value="">Any weapon or vehicle</option>
			{#each causeOptions as c (c.cause)}<option value={c.cause}>{c.label}</option>{/each}
		</select>
		<select class="input" aria-label="Filter by kind of kill" bind:value={filter.kind}>
			{#each KINDS as k (k.key)}<option value={k.key}>{k.label}</option>{/each}
		</select>
		<input
			class="input"
			type="number"
			min="1"
			step="1"
			placeholder="At least this many metres"
			aria-label="Minimum distance in metres"
			bind:value={filter.minM}
		/>
	</div>
	<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2 text-[12.5px] text-mist-600">
		<span class="text-mist-200">
			{#if total === null}{loading ? 'Counting…' : ''}{:else}{fmtNum(total)}
				{total === 1 ? 'kill' : 'kills'}{filtered ? ' match' : ''}{/if}
		</span>
		{#if filtered}<button class="btn btn-sm" onclick={clear}>Clear filters</button>{/if}
		<span class="ml-auto">newest first · new kills appear as the game posts them</span>
	</div>
	<div class="table-wrap">
		<table>
			<thead>
				<tr
					><th>When</th><th>Killer</th><th>Victim</th><th>Cause</th><th class="num">Distance</th><th
					></th></tr
				>
			</thead>
			<tbody>
				{#each kills as k (k.eventId)}
					<tr class={k.teamKill ? 'text-warn' : ''}>
						<td class="font-mono text-[12px] whitespace-nowrap text-mist-400">{fmtTime(k.ts)}</td>
						<td>
							{#if k.killer}
								<a href={dossier(k.killer.steamId)} class="hover:text-accent hover:underline"
									>{k.killer.name}</a
								>
								{#if k.killer.faction}<FactionChip
										faction={k.killer.faction}
										scores={status?.scores}
									/>{/if}
								<button
									type="button"
									class="block cursor-pointer font-mono text-[11px] text-mist-600 hover:text-accent"
									title="Only kills by this player"
									onclick={() => (filter.killer = k.killer?.steamId ?? '')}
									>{k.killer.steamId}</button
								>
							{:else}<span class="text-mist-600">—</span>{/if}
						</td>
						<td>
							<a href={dossier(k.victim.steamId)} class="hover:text-accent hover:underline"
								>{k.victim.name}</a
							>
							{#if k.victim.faction}<FactionChip
									faction={k.victim.faction}
									scores={status?.scores}
								/>{/if}
							<button
								type="button"
								class="block cursor-pointer font-mono text-[11px] text-mist-600 hover:text-accent"
								title="Only deaths of this player"
								onclick={() => (filter.victim = k.victim.steamId)}>{k.victim.steamId}</button
							>
						</td>
						<td class="text-mist-200">{causeText(k)}</td>
						<td class="num">{k.distanceM === null ? '—' : `${Math.round(k.distanceM)} m`}</td>
						<td class="whitespace-nowrap">
							{#if k.teamKill}<span class="chip">team kill</span>{/if}
							{#if k.suicide}<span class="chip">suicide</span>{/if}
							{#if k.headshot}<span class="chip">headshot</span>{/if}
							{#each k.tags as t (t)}<span class="chip"
									>{t.replace(/([a-z])([A-Z])/g, '$1 $2').toLowerCase()}</span
								>{/each}
						</td>
					</tr>
				{:else}
					<tr
						><td colspan="6" class="py-6 text-center text-mist-600">
							{#if loading}Loading…{:else if configured === false}This server has no kill feed yet.
								An org owner turns it on under
								<a
									href="/server/{encodeURIComponent(id)}/config"
									class="text-accent hover:underline">Config</a
								>.{:else if filtered}Nothing matches.{:else}No kills received yet. They appear here
								as the game posts them.{/if}
						</td></tr
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
	<p class="note">
		Every kill the game's feed delivered, kept for good. Names link to the player's dossier; a
		SteamID under a name narrows the list to that player. Team kills are inferred from the factions
		Warcon observed at the time.
	</p>
</div>
