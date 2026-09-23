<script lang="ts">
	// The organisation's ban list: its entries, where each stands on every server, and the add /
	// remove controls.
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { banUid } from '$lib/ban-message';
	import { describeSync, STATE_TEXT, STATE_TONE } from '$lib/lists';
	import Badge from '$lib/components/Badge.svelte';
	import BanDialog from '$lib/components/BanDialog.svelte';
	import EditBanDialog from '$lib/components/EditBanDialog.svelte';
	import BanMessagePanel from './BanMessagePanel.svelte';
	import ImportCandidates from './ImportCandidates.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { ListEntryView, ListSyncSummary, OrgListsView } from '$lib/types';

	let {
		entries,
		lists,
		org
	}: {
		entries: ListEntryView[];
		lists: OrgListsView;
		org: { id: string; name: string };
	} = $props();

	let path = $derived(`/api/orgs/${encodeURIComponent(org.id)}/lists/ban/entries`);
	let search = $state('');
	let busy = $state(false);
	let banning = $state(false);
	let editing = $state<ListEntryView | null>(null);
	let owner = $derived(lists.role === 'owner');

	/** newest first as the server sends them, until a header is clicked */
	const sort = new TableSort<ListEntryView>({
		player: { by: (e) => e.name || e.steamId },
		uid: { by: (e) => banUid(e.id) },
		reason: { by: (e) => e.reason },
		by: { by: (e) => e.addedByName },
		added: { by: (e) => e.addedAt, dir: 'desc' },
		// permanent bans last, then the soonest to lift first
		expires: { by: (e) => e.expiresAt ?? '\uffff' },
		servers: { by: (e) => e.servers.filter((s) => s.state === 'applied').length, dir: 'desc' }
	});
	let rows = $derived(
		sort.sorted(
			entries.filter((e) =>
				matches(search, e.steamId, e.name, banUid(e.id), e.reason, e.addedByName)
			)
		)
	);
	let dossierBase = $derived(
		lists.servers.length ? `/server/${encodeURIComponent(lists.servers[0].id)}/players` : null
	);

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

	async function remove(e: ListEntryView) {
		const label = e.name ? `${e.name} (${e.steamId})` : e.steamId;
		if (
			!(await confirmDialog(
				`Unban ${label} across ${org.name}? The panel lifts the ban on every server it applied it to.`,
				{ okLabel: 'Unban', danger: true }
			))
		)
			return;
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'DELETE',
				`${path}/${encodeURIComponent(e.steamId)}`
			);
			toast(describeSync(res.sync, `Unbanned ${e.steamId}.`), 'ok', 8000);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<div class="mb-4 flex flex-wrap items-center gap-3">
	<div>
		<h2 class="text-lg font-semibold tracking-tight">Ban list</h2>
		<p class="text-[13px] text-mist-400">
			Bans kept by the organisation and pushed to every one of its servers. Bans added on a server
			directly stay local to it.
		</p>
	</div>
	<span class="ml-auto inline-flex gap-1.5">
		<button class="btn" disabled={busy || !lists.servers.length} onclick={syncNow}>Sync now</button>
		<button class="btn btn-primary" onclick={() => (banning = true)}>Add ban</button>
	</span>
</div>

{#if lists.servers.length}
	<div class="mb-4 flex flex-wrap gap-2">
		{#each lists.servers as s (s.id)}
			<div
				class="rounded-ctl border border-black bg-ink-950 px-3 py-2 text-[12.5px] {s.lastError
					? 'border-l-2 border-l-danger'
					: ''}"
			>
				<div class="font-medium">{s.name}</div>
				<div class="text-mist-400">
					{#if s.syncedAt}synced {fmtTime(s.syncedAt)}{:else}never synced{/if}
				</div>
				{#if s.lastError}<div class="text-danger">{s.lastError}</div>{/if}
			</div>
		{/each}
	</div>
{/if}

{#if owner}<ImportCandidates kind="ban" {org} {owner} />{/if}

{#if !lists.servers.length}
	<div class="callout mb-4">
		{org.name} has no servers yet, so there is nothing to push the list to. Entries are kept and applied
		when a server is added.
	</div>
{/if}

{#if lists.banMessage !== null}<BanMessagePanel {org} banMessage={lists.banMessage} {owner} />{/if}

<div class="mb-3 flex flex-wrap items-center gap-2">
	<input
		class="input w-full sm:w-80"
		type="search"
		placeholder="Filter by name, SteamID, ban ID, reason, admin…"
		bind:value={search}
	/>
	<span class="text-[12.5px] text-mist-600"
		>{entries.length} ban{entries.length === 1 ? '' : 's'}</span
	>
</div>

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<SortHeader {sort} key="player">Player</SortHeader>
				<SortHeader {sort} key="uid">ID</SortHeader>
				<SortHeader {sort} key="reason">Reason</SortHeader>
				<SortHeader {sort} key="by">By</SortHeader>
				<SortHeader {sort} key="added">Added</SortHeader>
				<SortHeader {sort} key="expires">Expires</SortHeader>
				<SortHeader {sort} key="servers">Servers</SortHeader>
				<th></th>
			</tr>
		</thead>
		<tbody>
			{#each rows as e (e.id)}
				<tr class={e.expired ? 'text-mist-400' : ''}>
					<td>
						{#if dossierBase}
							<a href="{dossierBase}/{e.steamId}" class="font-medium text-accent hover:underline"
								>{e.name || e.steamId}</a
							>
						{:else}
							<span class="font-medium">{e.name || e.steamId}</span>
						{/if}
						{#if e.name}<div class="font-mono text-[12px] text-mist-600">{e.steamId}</div>{/if}
					</td>
					<td><span class="chip">{banUid(e.id)}</span></td>
					<td class="max-w-[280px]">
						{#if e.reason}{e.reason}{:else}<span class="text-mist-600">—</span>{/if}
					</td>
					<td>{e.addedByName || '—'}</td>
					<td class="text-[12.5px] whitespace-nowrap text-mist-400">{fmtTime(e.addedAt)}</td>
					<td class="text-[12.5px] whitespace-nowrap">
						{#if !e.expiresAt}
							<span class="text-mist-600">never</span>
						{:else if e.expired}
							<Badge tone="warn">expired, lifting</Badge>
						{:else}
							{fmtTime(e.expiresAt)}
						{/if}
					</td>
					<td>
						<span class="inline-flex flex-wrap gap-1">
							{#each e.servers as s (s.serverId)}
								<span title="{s.serverName}: {STATE_TEXT[s.state]}{s.error ? ` — ${s.error}` : ''}">
									<Badge tone={STATE_TONE[s.state]}>{s.serverName}</Badge>
								</span>
							{/each}
						</span>
					</td>
					<td class="text-right whitespace-nowrap">
						<button class="btn btn-sm" disabled={busy} onclick={() => (editing = e)}>Edit</button>
						<button class="btn btn-sm btn-danger" disabled={busy} onclick={() => remove(e)}
							>Unban</button
						>
					</td>
				</tr>
			{:else}
				<tr
					><td colspan="8" class="py-6 text-center text-mist-600"
						>{entries.length ? 'Nothing matches the filter.' : 'No bans yet.'}</td
					></tr
				>
			{/each}
		</tbody>
	</table>
</div>

<p class="note">
	Server badges: <Badge tone="ok">applied</Badge> by the panel, <Badge tone="warn">pending</Badge> the
	next sync, <Badge tone="err">failed</Badge> (hover for why), <Badge>local</Badge> already on that server
	but added outside the panel, so the panel never removes it.
</p>

{#if editing}
	<EditBanDialog
		path="{path}/{encodeURIComponent(editing.steamId)}"
		who={editing.name || editing.steamId}
		placed={`Banned across ${org.name}${editing.addedByName ? ` by ${editing.addedByName}` : ''} on ${fmtTime(editing.addedAt)}.`}
		reason={editing.reason}
		expiresAt={editing.expiresAt}
		onclose={() => (editing = null)}
		ondone={() => invalidateAll()}
	/>
{/if}

{#if banning}
	<BanDialog
		orgId={org.id}
		orgName={org.name}
		canOrg
		banMessage={lists.banMessage}
		onclose={() => (banning = false)}
		ondone={() => invalidateAll()}
	/>
{/if}
