<script lang="ts">
	// Bans or reserved slots the organisation's servers already hold that its list does not: a
	// callout with the count and, for owners, the review-and-import dialog. Shared by both list
	// pages.
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import { describeSync } from '$lib/lists';
	import Badge from '$lib/components/Badge.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { matches } from '$lib/table.svelte';
	import type { ImportCandidate, ListKind, ListSyncSummary } from '$lib/types';

	let { kind, org, owner }: { kind: ListKind; org: { id: string; name: string }; owner: boolean } =
		$props();

	let candidates = $state<ImportCandidate[] | null>(null);
	let importing = $state(false);
	let busy = $state(false);
	let picked = $state<Record<string, boolean>>({});
	let mine = $derived((candidates ?? []).filter((c) => c.kind === kind));
	let search = $state('');
	/** the candidates the dialog shows; Select all / none act on these */
	let shown = $derived(
		mine.filter((c) =>
			matches(
				search,
				c.name,
				c.steamId,
				...c.servers.flatMap((s) => [s.serverName, s.reason, s.bannedBy])
			)
		)
	);
	let noun = $derived(kind === 'ban' ? 'ban' : 'reserved slot');
	let path = $derived(`/api/orgs/${encodeURIComponent(org.id)}/lists/import`);

	async function refresh() {
		try {
			candidates = (await api<{ candidates: ImportCandidate[] }>('GET', path)).candidates;
		} catch (err) {
			console.warn('import candidates', err);
		}
	}
	$effect(() => {
		void kind;
		void refresh();
	});

	function open() {
		const next: Record<string, boolean> = {};
		for (const c of mine) next[c.steamId] = true;
		picked = next;
		importing = true;
	}
	async function run() {
		const entries = mine
			.filter((c) => picked[c.steamId])
			.map((c) => ({ kind: c.kind, steamId: c.steamId }));
		if (!entries.length) {
			importing = false;
			return;
		}
		busy = true;
		try {
			const res = await api<{ imported: number; sync: ListSyncSummary }>('POST', path, {
				entries
			});
			toast(describeSync(res.sync, `Imported ${res.imported}.`), 'ok', 8000);
			importing = false;
			candidates = null;
			await invalidateAll();
			await refresh();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

{#if mine.length}
	<div class="callout mb-4 flex flex-wrap items-center gap-3">
		<span
			><b>{mine.length} {noun}{mine.length === 1 ? '' : 's'}</b> found on your servers that
			{mine.length === 1 ? 'is' : 'are'} not on the organisation list.
			{#if owner}Import {mine.length === 1 ? 'it' : 'them'} to manage
				{mine.length === 1 ? 'it' : 'them'} from here and apply
				{mine.length === 1 ? 'it' : 'them'} everywhere.{:else}An owner of {org.name} can import them.{/if}</span
		>
		{#if owner}
			<button class="ml-auto btn btn-sm" onclick={open}>Review and import</button>
		{/if}
	</div>
{/if}

{#if importing}
	<Modal title="Import {noun}s from your servers" wide onclose={() => (importing = false)}>
		<p class="mb-3 text-[13px] text-mist-400">
			These {noun}s exist on the servers below but not on the organisation list. Importing puts them
			on the list, marks them as managed where they already exist, and applies them to every other
			server in {org.name}. Removing an imported entry later lifts it everywhere the panel manages
			it.
		</p>
		{#if mine.length > 5}
			<input
				class="mb-3 input w-full sm:w-72"
				type="search"
				placeholder="Filter by name, SteamID, server…"
				aria-label="Filter candidates"
				bind:value={search}
			/>
		{/if}
		<div class="max-h-[50vh] table-wrap overflow-y-auto">
			<table>
				<thead>
					<tr>
						<th></th>
						<th>Player</th>
						<th>On</th>
						{#if kind === 'ban'}<th>Reason</th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each shown as c (c.steamId)}
						<tr>
							<td><input type="checkbox" bind:checked={picked[c.steamId]} /></td>
							<td>
								<span class="font-medium">{c.name || c.steamId}</span>
								{#if c.name}<div class="font-mono text-[12px] text-mist-600">{c.steamId}</div>{/if}
							</td>
							<td>
								<span class="inline-flex flex-wrap gap-1">
									{#each c.servers as s (s.serverId)}<Badge>{s.serverName}</Badge>{/each}
								</span>
							</td>
							{#if kind === 'ban'}
								<td class="max-w-[280px] text-[12.5px]">
									{#each c.servers.filter((s) => s.reason || s.bannedBy) as s (s.serverId)}
										<div>
											{s.reason || '—'}{#if s.bannedBy}
												<span class="text-mist-600">by {s.bannedBy}</span>{/if}
										</div>
									{/each}
								</td>
							{/if}
						</tr>
					{:else}
						<tr><td colspan="4" class="py-6 text-center text-mist-600">Nobody matches.</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
		<div class="mt-4 flex flex-wrap justify-end gap-2">
			<button
				type="button"
				class="mr-auto btn"
				onclick={() => {
					const all = shown.every((c) => picked[c.steamId]);
					const next = { ...picked };
					for (const c of shown) next[c.steamId] = !all;
					picked = next;
				}}>{shown.every((c) => picked[c.steamId]) ? 'Select none' : 'Select all'}</button
			>
			<button type="button" class="btn" data-close onclick={() => (importing = false)}
				>Cancel</button
			>
			<button type="button" class="btn btn-primary" disabled={busy} onclick={run}
				>Import {mine.filter((c) => picked[c.steamId]).length}</button
			>
		</div>
	</Modal>
{/if}
