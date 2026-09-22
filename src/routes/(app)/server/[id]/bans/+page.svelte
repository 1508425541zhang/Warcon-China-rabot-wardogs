<script lang="ts">
	// This server's ban list: what the game server holds, with what the organisation's list and the
	// server's own list contribute marked out, and the bans those lists still wait to place.
	import { invalidateAll } from '$app/navigation';
	import { api, rconGet, rconPost, errorMessage } from '$lib/api';
	import { fmtSpan, fmtTime } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import BanDialog from '$lib/components/BanDialog.svelte';
	import EditBanDialog from '$lib/components/EditBanDialog.svelte';
	import SteamName from '$lib/components/SteamName.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import { steamProfiles, type SteamProfile } from '$lib/steam-profiles';
	import { describeSync } from '$lib/lists';
	import type { Ban, ListSyncServer, ListSyncSummary, ServerListsState } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let admin = $derived(can(data.server.caps, 'bans.manage'));
	let listsEdit = $derived(can(data.server.caps, 'lists.ban'));
	let orgPath = $derived(`/orgs/${encodeURIComponent(data.server.orgId)}`);

	let listState = $state<ServerListsState | null>(null);
	$effect(() => {
		listState = data.listState;
	});
	let bans = $state<Ban[]>([]);
	/** Steam personas for the ids on the list, where a key is configured */
	let steam = $state<Record<string, SteamProfile | null>>({});
	let banSearch = $state('');
	let selectedBan = $state<string | null>(null);
	let busy = $state(false);
	let banning = $state(false);

	let editing = $state(false);

	/** One line of the table: a ban on the panel's lists, or one the game holds in its own list. */
	interface Row {
		steamId: string;
		source: 'org' | 'here' | 'local';
		bannedAt: string;
		by: string;
		reason: string;
		expiresAt: string | null;
	}
	const utc = (iso: string) => iso.slice(0, 16).replace('T', ' ');
	let rows = $derived.by(() => {
		const held = new Set(bans.map((b) => b.steamId));
		const row = (steamId: string, b: Ban | null): Row => {
			const src = banSource(steamId);
			const managed = !!src?.managed;
			return {
				steamId,
				source: !managed ? 'local' : src.scope === 'server' ? 'here' : 'org',
				bannedAt: (managed && src.addedAt ? utc(src.addedAt) : '') || utc(b?.bannedAtUtc ?? ''),
				by: (managed && src.addedByName) || b?.bannedBy || '',
				reason: (managed && src.reason) || b?.reason || '',
				expiresAt: managed ? src.expiresAt : null
			};
		};
		return [
			...bans.map((b) => row(b.steamId, b)),
			...Object.entries(listState?.bans ?? {})
				.filter(([steamId, s]) => s.managed && !held.has(steamId))
				.map(([steamId]) => row(steamId, null))
		];
	});

	const sort = new TableSort<Row>({
		player: { by: (r) => steam[r.steamId]?.name || r.steamId },
		source: { by: (r) => r.source },
		bannedAt: { by: (r) => r.bannedAt, dir: 'desc' },
		by: { by: (r) => r.by },
		reason: { by: (r) => r.reason },
		expires: { by: (r) => r.expiresAt ?? '\uffff' }
	});
	let banRows = $derived(
		sort.sorted(
			rows.filter((r) => matches(banSearch, r.steamId, steam[r.steamId]?.name, r.by, r.reason))
		)
	);
	let selectedRow = $derived(rows.find((r) => r.steamId === selectedBan) ?? null);
	/** A ban on the server's own list is edited with Bans, one on the org's with the org ban list. */
	let canEdit = $derived(
		!!selectedRow &&
			((selectedRow.source === 'here' && admin) ||
				(selectedRow.source === 'org' && !!listState?.canEditOrgBans))
	);
	let entryPath = $derived(
		!selectedRow
			? ''
			: selectedRow.source === 'here'
				? `/api/servers/${encodeURIComponent(id)}/lists/ban/entries/${selectedRow.steamId}`
				: `/api/orgs/${encodeURIComponent(data.server.orgId)}/lists/ban/entries/${selectedRow.steamId}`
	);
	let orgBanCount = $derived(
		data.orgLists?.lists.find((l) => l.kind === 'ban')?.entryCount ?? null
	);
	let managedBans = $derived(
		Object.values(listState?.bans ?? {}).filter(
			(s) => s.managed && s.scope === 'org' && s.state === 'applied'
		).length
	);
	let pendingCount = $derived(
		Object.values(listState?.bans ?? {}).filter(
			(s) => s.state === 'pending' || s.state === 'failed'
		).length
	);

	async function act(
		action: string,
		params: object,
		opts: { confirm?: string; danger?: boolean; after?: () => Promise<unknown> } = {}
	) {
		if (
			opts.confirm &&
			!(await confirmDialog(opts.confirm, { okLabel: 'Do it', danger: opts.danger }))
		)
			return null;
		try {
			const result = await rconPost<{ message?: string }>(id, action, params);
			toast(result?.message || `${action} done.`, 'ok');
			if (opts.after) await opts.after();
			return result;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return null;
		}
	}
	async function refreshListState() {
		try {
			listState = await api<ServerListsState>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/lists/state`
			);
		} catch (err) {
			console.warn('list state', err);
		}
	}
	async function refreshBans() {
		bans = (await rconGet<{ bans: Ban[] }>(id, 'bans')).bans;
		await refreshListState();
		void lookupSteam(rows.map((r) => r.steamId));
	}
	async function lookupSteam(ids: string[]) {
		const found = await steamProfiles(ids.filter((s) => !(s in steam)));
		if (Object.keys(found).length) steam = { ...steam, ...found };
	}
	const refreshAll = () => Promise.all([refreshBans(), invalidateAll()]);

	$effect(() => {
		void id;
		refreshBans().catch((err) => toast(errorMessage(err), 'err'));
	});
	const banSource = (steamId: string) => listState?.bans[steamId] ?? null;

	async function syncNow() {
		busy = true;
		try {
			const res = await api<{ sync: ListSyncServer }>(
				'POST',
				`/api/servers/${encodeURIComponent(id)}/lists/sync`
			);
			toast(describeSync({ servers: [res.sync] }, 'Sync ran.'), 'ok', 8000);
			await refreshAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	/**
	 * Put a local ban on the org list. Owners import it (the panel then manages it here too);
	 * editors add it to the list, and this server's copy stays local.
	 */
	async function promoteSelected() {
		if (!selectedBan || !listState) return;
		const steamId = selectedBan;
		const ban = bans.find((b) => b.steamId === steamId);
		const lists = `/api/orgs/${encodeURIComponent(listState.orgId)}/lists`;
		busy = true;
		try {
			const res = listState.orgOwner
				? await api<{ sync: ListSyncSummary }>('POST', `${lists}/import`, {
						entries: [{ kind: 'ban', steamId, reason: ban?.reason ?? '' }]
					})
				: await api<{ sync: ListSyncSummary }>('POST', `${lists}/ban/entries`, {
						steamId,
						reason: ban?.reason ?? ''
					});
			toast(describeSync(res.sync, `${steamId} is on the organisation's ban list.`), 'ok', 8000);
			await refreshAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	/** A ban on the server's own list is lifted by withdrawing the entry; the sync unbans. */
	async function liftHere(steamId: string) {
		if (!(await confirmDialog(`Lift the ban on ${steamId}?`, { okLabel: 'Do it' }))) return;
		busy = true;
		try {
			const res = await api<{ sync: ListSyncServer }>(
				'DELETE',
				`/api/servers/${encodeURIComponent(id)}/lists/ban/entries/${steamId}`
			);
			toast(describeSync({ servers: [res.sync] }, `Lifted the ban on ${steamId}.`), 'ok', 8000);
			selectedBan = null;
			await refreshBans();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function unbanSelected() {
		if (!selectedBan) return;
		const src = banSource(selectedBan);
		if (src?.managed && src.scope === 'server') return liftHere(selectedBan);
		const confirm = src?.managed
			? `${selectedBan} is banned by the organisation's ban list, so the panel will ban them again at the next sync. Unban here anyway? To lift it everywhere, remove it from the organisation's ban list instead.`
			: `Unban ${selectedBan}?`;
		await act(
			'unban',
			{ steamId: selectedBan },
			{
				confirm,
				danger: !!src?.managed,
				after: async () => {
					selectedBan = null;
					await refreshBans();
				}
			}
		);
	}
</script>

<div class="mb-4 panel">
	<div class="mb-2 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0!">Organisation lists · {data.server.orgName}</span>
		<span class="ml-auto inline-flex flex-wrap gap-1.5">
			{#if listState?.canEditOrgBans}
				<a class="btn btn-sm" href="{orgPath}/bans">Ban list</a>
			{/if}
			{#if listsEdit}
				<button class="btn btn-sm" disabled={busy} onclick={syncNow}>Sync now</button>
			{/if}
		</span>
	</div>
	<div class="flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
		{#if orgBanCount !== null}
			<span
				><b>{orgBanCount}</b> org ban{orgBanCount === 1 ? '' : 's'}, <b>{managedBans}</b> applied here</span
			>
		{:else}
			<span class="text-mist-400"
				>Managed by the organisation's owners and server admins; entries they push here are marked
				<Badge tone="ok">org</Badge> below.</span
			>
		{/if}
		{#if pendingCount}<Badge tone="warn">{pendingCount} pending or failed</Badge>{/if}
	</div>
	<div class="mt-1 text-[12.5px] text-mist-400">
		{#if listState?.sync?.syncedAt}
			Last synced {fmtTime(listState.sync.syncedAt)}.
		{:else}
			Not synced yet.
		{/if}
		{#if listState?.sync?.lastError}<span class="text-danger">
				{listState.sync.lastError}</span
			>{/if}
	</div>
</div>

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0!">Bans on this server</span>
		<button class="ml-auto btn btn-sm btn-danger" disabled={!admin} onclick={() => (banning = true)}
			>Ban a SteamID</button
		>
	</div>
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<div class="join w-full sm:w-auto sm:min-w-[320px]">
			<input
				class="input"
				type="search"
				placeholder="Filter bans by SteamID, admin, reason…"
				bind:value={banSearch}
			/>
			<button class="btn" onclick={refreshBans}>Refresh</button>
		</div>
		<span class="inline-flex gap-1.5 sm:ml-auto">
			{#if selectedBan && listState?.canEditOrgBans && !banSource(selectedBan)?.managed}
				<button class="btn" disabled={busy} onclick={promoteSelected}
					>{listState.orgOwner ? 'Promote to org list' : 'Add to org list'}</button
				>
			{/if}
			<button class="btn" disabled={busy || !canEdit} onclick={() => (editing = true)}>Edit</button>
			<button
				class="btn btn-danger"
				disabled={busy || !admin || !selectedBan}
				onclick={unbanSelected}>Unban selected</button
			>
		</span>
	</div>
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<SortHeader {sort} key="player">Player</SortHeader>
					<SortHeader {sort} key="source">Source</SortHeader>
					<SortHeader {sort} key="bannedAt">Banned at (UTC)</SortHeader>
					<SortHeader {sort} key="by">By</SortHeader>
					<SortHeader {sort} key="reason">Reason</SortHeader>
					<SortHeader {sort} key="expires">Expires</SortHeader>
				</tr>
			</thead>
			<tbody>
				{#each banRows as b (b.steamId)}
					<tr
						class="clickable {selectedBan === b.steamId ? 'selected' : ''}"
						onclick={() => (selectedBan = selectedBan === b.steamId ? null : b.steamId)}
					>
						<td>
							<SteamName profile={steam[b.steamId]} class="max-w-[240px] font-medium" />
							<a
								href="/server/{encodeURIComponent(id)}/players/{b.steamId}"
								class="block font-mono text-[12.5px] hover:text-accent hover:underline"
								title="Open dossier"
								onclick={(e) => e.stopPropagation()}>{b.steamId}</a
							>
						</td>
						<td>
							{#if b.source === 'local'}
								<Badge>local</Badge>
							{:else}
								<Badge tone="ok">{b.source}</Badge>
							{/if}
						</td>
						<td class="font-mono text-[12px] whitespace-nowrap text-mist-400"
							>{b.bannedAt || '—'}</td
						>
						<td>{b.by}</td>
						<td
							>{#if b.reason}{b.reason}{:else}<span class="text-mist-600">—</span>{/if}</td
						>
						<td class="whitespace-nowrap">
							{#if b.source === 'local'}
								<span class="text-mist-600">—</span>
							{:else if b.expiresAt}
								<div class="text-accent">
									in {fmtSpan(new Date(b.expiresAt).getTime() - Date.now())}
								</div>
								<div class="font-mono text-[12px] text-mist-400">{fmtTime(b.expiresAt)}</div>
							{:else}
								<span class="text-mist-400">Permanent</span>
							{/if}
						</td>
					</tr>
				{:else}
					<tr><td colspan="6" class="py-6 text-center text-mist-600">No bans.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="note">
		<Badge tone="ok">org</Badge> bans come from the organisation's ban list and
		<Badge tone="ok">here</Badge> bans are on this server's own list. The panel enforces both itself:
		a banned player is removed the moment they are seen on the server, and nothing is written to the game's
		files, so an unban or an expiry takes effect at once. <Badge>local</Badge> bans are held by the game
		in its own list; the panel leaves them alone, and some hosts only forget one when it is taken out
		of the server's settings file.
	</p>
</div>

{#if editing && selectedRow}
	<EditBanDialog
		path={entryPath}
		who={steam[selectedRow.steamId]?.name || selectedRow.steamId}
		placed={`Banned ${selectedRow.source === 'here' ? `on ${data.server.name}` : `across ${data.server.orgName}`}${selectedRow.by ? ` by ${selectedRow.by}` : ''}${selectedRow.bannedAt ? ` on ${selectedRow.bannedAt} UTC` : ''}.`}
		reason={selectedRow.reason}
		expiresAt={selectedRow.expiresAt}
		onclose={() => (editing = false)}
		ondone={refreshBans}
	/>
{/if}

{#if banning}
	<BanDialog
		orgId={data.server.orgId}
		orgName={data.server.orgName}
		server={{ id, name: data.server.name }}
		canOrg={listState?.canEditOrgBans ?? false}
		banMessage={listState?.banMessage}
		onclose={() => (banning = false)}
		ondone={refreshAll}
	/>
{/if}
