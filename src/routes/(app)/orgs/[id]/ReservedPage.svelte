<script lang="ts">
	// The organisation's reserved-slot list, laid out like a server's Reserved slots tab: how many
	// hold a slot and who is playing right now, where the list stands on each server, the form to
	// hand out a slot everywhere, and the roster itself.
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { watchLive } from '$lib/live';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { describeSync, STATE_TEXT, STATE_TONE } from '$lib/lists';
	import { isSteamId, steamProfiles, type SteamProfile } from '$lib/steam-profiles';
	import Badge from '$lib/components/Badge.svelte';
	import SteamName from '$lib/components/SteamName.svelte';
	import ImportCandidates from './ImportCandidates.svelte';
	import type { ListEntryView, ListSyncSummary, OrgListsView } from '$lib/types';

	let {
		entries,
		lists,
		org,
		servers
	}: {
		entries: ListEntryView[];
		lists: OrgListsView;
		org: { id: string; name: string };
		/** the org's servers this user may open: who is playing is watched on these */
		servers: { id: string; name: string }[];
	} = $props();

	let path = $derived(`/api/orgs/${encodeURIComponent(org.id)}/lists/reserve/entries`);
	let owner = $derived(lists.role === 'owner');
	let search = $state('');
	let busy = $state(false);
	let newId = $state('');
	let newReason = $state('');
	/** Steam personas, for the avatar beside a name */
	let steam = $state<Record<string, SteamProfile | null>>({});
	/** the persona for the id being typed into the reserve form: undefined while unknown */
	let preview = $state<SteamProfile | null | undefined>(undefined);
	let previewId = $derived(isSteamId(newId.trim()) ? newId.trim() : '');
	/** the worker's latest player list per server, by server id */
	let playersOn = $state<Record<string, { steamId: string; name: string }[]>>({});
	/** who is playing right now, by SteamID: the name they play under and where */
	let online = $derived.by(() => {
		const out: Record<string, { name: string; servers: string[] }> = {};
		for (const s of servers)
			for (const p of playersOn[s.id] ?? [])
				(out[p.steamId] ??= { name: p.name, servers: [] }).servers.push(s.name);
		return out;
	});

	let picked = $derived(entries.filter((e) => !e.member).length);
	let members = $derived(entries.length - picked);
	let playing = $derived(entries.filter((e) => e.steamId in online).length);
	/** where the list stands on each server: how many entries the panel has applied there */
	let standing = $derived(
		lists.servers.map((s) => {
			const states = entries.map((e) => e.servers.find((x) => x.serverId === s.id)?.state);
			return {
				...s,
				applied: states.filter((st) => st === 'applied' || st === 'local').length,
				pending: states.filter((st) => st === 'pending' || st === 'failed').length,
				href: servers.some((x) => x.id === s.id)
					? `/server/${encodeURIComponent(s.id)}/slots`
					: null
			};
		})
	);
	let dossierBase = $derived(
		servers.length ? `/server/${encodeURIComponent(servers[0].id)}/players` : null
	);

	/**
	 * The roster: people playing right now first, then slots handed out by hand, then members.
	 */
	let rows = $derived.by(() => {
		const q = search.trim().toLowerCase();
		return entries
			.map((e) => {
				const live = online[e.steamId];
				return {
					e,
					live,
					name: live?.name ?? e.name ?? steam[e.steamId]?.name ?? null,
					avatar: steam[e.steamId]?.avatar ?? ''
				};
			})
			.filter(
				(r) =>
					!q ||
					r.e.steamId.includes(q) ||
					(r.name ?? '').toLowerCase().includes(q) ||
					r.e.reason.toLowerCase().includes(q) ||
					r.e.addedByName.toLowerCase().includes(q)
			)
			.sort(
				(a, b) =>
					Number(!!b.live) - Number(!!a.live) ||
					Number(a.e.member) - Number(b.e.member) ||
					(a.name ?? '￿').localeCompare(b.name ?? '￿') ||
					a.e.steamId.localeCompare(b.e.steamId)
			);
	});

	$effect(() => {
		const ids = servers.map((s) => s.id);
		return watchLive(ids, (v) => {
			playersOn[v.serverId] = v.ok
				? v.players.map((p) => ({ steamId: p.steamId, name: p.name }))
				: [];
		});
	});
	// Personas are looked up for the list's ids alone, so a lookup never re-runs on its own result.
	let listIds = $derived(
		entries
			.map((e) => e.steamId)
			.sort()
			.join(',')
	);
	$effect(() => {
		const ids = listIds.split(',').filter(Boolean);
		untrack(() => void lookupSteam(ids));
	});
	async function lookupSteam(ids: string[]) {
		const found = await steamProfiles(ids.filter((s) => !(s in steam)));
		if (Object.keys(found).length) steam = { ...steam, ...found };
	}
	$effect(() => {
		const want = previewId;
		preview = undefined;
		if (!want) return;
		void steamProfiles([want]).then((r) => {
			if (previewId === want && want in r) preview = r[want];
		});
	});

	async function addReserved() {
		const steamId = newId.trim();
		if (!isSteamId(steamId)) {
			toast('Enter a 17-digit SteamID64.', 'err');
			return;
		}
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>('POST', path, {
				steamId,
				reason: newReason.trim()
			});
			toast(describeSync(res.sync, `Reserved a slot for ${steamId}.`), 'ok', 8000);
			newId = '';
			newReason = '';
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function setMembersReserved(on: boolean) {
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'PATCH',
				`/api/orgs/${encodeURIComponent(org.id)}`,
				{ membersReserved: on }
			);
			toast(
				describeSync(res.sync, on ? 'Members now get a reserved slot.' : 'Member slots withdrawn.'),
				'ok',
				8000
			);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function syncNow() {
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'POST',
				`/api/orgs/${encodeURIComponent(org.id)}/lists/sync`
			);
			toast(describeSync(res.sync, 'Sync ran.'), 'ok', 8000);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function remove(e: ListEntryView, name: string | null) {
		const label = name ? `${name} (${e.steamId})` : e.steamId;
		if (
			!(await confirmDialog(`Withdraw the reserved slot for ${label} across ${org.name}?`, {
				okLabel: 'Withdraw',
				danger: true
			}))
		)
			return;
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'DELETE',
				`${path}/${encodeURIComponent(e.steamId)}`
			);
			toast(describeSync(res.sync, `Withdrew the slot for ${e.steamId}.`), 'ok', 8000);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<div class="mb-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
	<div class="panel">
		<span class="label-sm">On the list</span>
		<div class="flex items-baseline gap-2">
			<span class="font-display text-[34px] leading-none font-semibold tabular"
				>{entries.length}</span
			>
			{#if playing}
				<span class="ml-auto inline-flex items-center gap-1.5 text-[12.5px] text-ok"
					><span class="size-1.5 rounded-full bg-ok"></span>{playing} playing now</span
				>
			{/if}
		</div>
		<div class="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
			<span><b>{picked}</b> reserved by hand</span>
			{#if lists.membersReserved || members}<span><b>{members}</b> members</span>{/if}
		</div>
		<p class="note">
			Anyone on the list skips the join queue on every server in {org.name}. The list has no length
			limit; each server's MaxReservedSlots only sets how many player slots it holds back.
		</p>
	</div>

	<div class="panel">
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<span class="label-sm mb-0!">On the servers</span>
			<button class="ml-auto btn btn-sm" disabled={busy || !lists.servers.length} onclick={syncNow}
				>Sync now</button
			>
		</div>
		{#each standing as s (s.id)}
			<div class="kv items-start py-1.5 text-[13px]">
				<div class="min-w-0">
					{#if s.href}
						<a href={s.href} class="font-medium hover:text-accent hover:underline">{s.name}</a>
					{:else}
						<span class="font-medium">{s.name}</span>
					{/if}
					<div class="text-[12px] text-mist-400">
						{#if s.syncedAt}synced {fmtTime(s.syncedAt)}{:else}never synced{/if}
						{#if s.lastError}<span class="text-danger"> · {s.lastError}</span>{/if}
					</div>
				</div>
				<span class="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap">
					<span><b>{s.applied}</b> <span class="text-mist-400">of {entries.length}</span></span>
					{#if s.pending}<Badge tone="warn">{s.pending} pending</Badge>{/if}
				</span>
			</div>
		{:else}
			<p class="text-[13px] text-mist-400">
				{org.name} has no servers yet, so there is nothing to push the list to. Entries are kept and applied
				when a server is added.
			</p>
		{/each}
	</div>

	<div class="flex flex-col panel">
		<span class="label-sm">Reserve a slot everywhere</span>
		<form
			class="space-y-2"
			onsubmit={(e) => {
				e.preventDefault();
				void addReserved();
			}}
		>
			<div class="join join-wrap w-full">
				<input
					class="input font-mono"
					type="text"
					inputmode="numeric"
					placeholder="SteamID64…"
					maxlength="17"
					required
					bind:value={newId}
				/>
				<button type="submit" class="btn btn-primary" disabled={busy || !newId.trim()}
					>Reserve</button
				>
			</div>
			{#if previewId && preview}
				<div class="text-[12.5px]"><SteamName profile={preview} /></div>
			{:else if previewId && preview === null}
				<div class="text-[12.5px] text-mist-600">No Steam profile for that id.</div>
			{/if}
			<input
				class="input"
				type="text"
				maxlength="200"
				placeholder="Note, e.g. donor, clan member (optional)"
				bind:value={newReason}
			/>
		</form>
		<p class="note">
			Handed out on every server in {org.name}, now and when one is added later. To reserve a slot
			on one server only, use that server's Reserved slots tab.
		</p>
		{#if owner}
			<label class="mt-3 flex items-start gap-2 border-t border-white/8 pt-3 text-[13px]">
				<input
					type="checkbox"
					class="mt-0.5"
					checked={lists.membersReserved}
					disabled={busy}
					onchange={(e) => setMembersReserved((e.currentTarget as HTMLInputElement).checked)}
				/>
				<span
					><b>Members get a reserved slot.</b>
					<span class="block text-mist-400"
						>Every member of {org.name} who linked a SteamID on their Account page is reserved a slot
						on all its servers. Banned members are skipped.</span
					></span
				>
			</label>
		{/if}
	</div>
</div>

<ImportCandidates kind="reserve" {org} {owner} />

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0!">Who holds a slot</span>
		<div class="flex w-full items-center gap-2 sm:ml-auto sm:w-auto sm:min-w-[320px]">
			<input
				class="input"
				type="search"
				placeholder="Filter by name, SteamID, note, admin…"
				bind:value={search}
			/>
			<span class="shrink-0 text-[12.5px] whitespace-nowrap text-mist-600"
				>{entries.length} slot{entries.length === 1 ? '' : 's'}</span
			>
		</div>
	</div>
	{#if entries.length}
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<th>Player</th>
						<th>SteamID64</th>
						<th>Source</th>
						<th>Note</th>
						<th>Added</th>
						<th>Servers</th>
						<th></th>
					</tr>
				</thead>
				<tbody>
					{#each rows as r (r.e.id)}
						{@const e = r.e}
						<tr>
							<td>
								<span class="inline-flex min-w-0 items-center gap-2.5">
									<span
										class="size-2 shrink-0 rounded-full {r.live
											? 'bg-ok ring-[3px] ring-ok/25'
											: 'bg-ink-700'}"
										title={r.live
											? `Playing now on ${r.live.servers.join(', ')}`
											: 'Not on a server right now'}
									></span>
									{#if r.avatar}<img
											src={r.avatar}
											alt=""
											class="size-5 shrink-0 rounded-sm"
											loading="lazy"
											referrerpolicy="no-referrer"
										/>{/if}
									{#if dossierBase}
										<a
											href="{dossierBase}/{e.steamId}"
											class="truncate font-medium hover:text-accent hover:underline {r.name
												? ''
												: 'text-mist-400 italic'}"
											title="Open dossier">{r.name ?? 'Not seen yet'}</a
										>
									{:else}
										<span class="truncate font-medium {r.name ? '' : 'text-mist-400 italic'}"
											>{r.name ?? 'Not seen yet'}</span
										>
									{/if}
								</span>
								{#if r.live}<div class="mt-0.5 pl-[18px] caps text-[10px] text-ok">
										playing on {r.live.servers.join(', ')}
									</div>{/if}
							</td>
							<td class="font-mono text-[12.5px] text-mist-400">{e.steamId}</td>
							<td>
								{#if e.member}
									<Badge tone="accent">member</Badge>
								{:else}
									<Badge tone="ok">list</Badge>
								{/if}
							</td>
							<td class="max-w-[280px]">
								{#if e.reason}{e.reason}{:else}<span class="text-mist-600">—</span>{/if}
							</td>
							<td class="text-[12.5px] whitespace-nowrap">
								{#if e.member}
									<span class="text-mist-400">by membership</span>
								{:else}
									<div>{e.addedByName || '—'}</div>
									<div class="text-mist-400">{fmtTime(e.addedAt)}</div>
								{/if}
							</td>
							<td>
								<span class="inline-flex flex-wrap gap-1">
									{#each e.servers as s (s.serverId)}
										<span
											class="whitespace-nowrap"
											title="{s.serverName}: {STATE_TEXT[s.state]}{s.error ? ` — ${s.error}` : ''}"
										>
											<Badge tone={STATE_TONE[s.state]}>{s.serverName}</Badge>
										</span>
									{/each}
								</span>
							</td>
							<td class="text-right whitespace-nowrap">
								{#if !e.member}
									<button
										type="button"
										class="btn btn-sm btn-ghost"
										title="Withdraw this slot on every server"
										disabled={busy}
										onclick={() => remove(e, r.name)}>Withdraw</button
									>
								{/if}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="7" class="py-6 text-center text-mist-600"
								>Nobody matches that filter.</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="note">
			<Badge tone="ok">list</Badge> slots were reserved here and can be withdrawn; <Badge
				tone="accent">member</Badge
			> slots follow membership and go when the member leaves or the switch above is turned off. Server
			badges: <Badge tone="ok">applied</Badge> by the panel, <Badge tone="warn">pending</Badge> the next
			sync, <Badge tone="err">failed</Badge> (hover for why), <Badge>local</Badge> already on that server
			but added outside the panel, so the panel never removes it.
		</p>
	{:else}
		<div class="callout mb-0">
			<b>Nobody holds a reserved slot yet.</b>
			<span class="block text-mist-400"
				>A reserved slot lets your admins, donors and clan members skip the queue when a server is
				full. Reserve one above and the panel hands it out on every server in {org.name}.</span
			>
		</div>
	{/if}
</div>
