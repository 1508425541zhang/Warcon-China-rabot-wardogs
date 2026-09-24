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
	let noun = $derived(kind === 'ban' ? '封禁' : '预留席位');
	let path = $derived(`/api/orgs/${encodeURIComponent(org.id)}/lists/import`);

	async function refresh() {
		try {
			candidates = (await api<{ candidates: ImportCandidate[] }>('GET', path)).candidates;
		} catch (err) {
			console.warn('导入候选条目', err);
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
			toast(describeSync(res.sync, `已导入 ${res.imported} 条。`), 'ok', 8000);
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
		<span>
			在你的服务器上发现 <b>{mine.length} 条{noun}</b>，尚未加入组织列表。
			{#if owner}导入后会由面板管理，并同步至组织内其他服务器。{:else}请联系“{org.name}”的所有者导入。{/if}
		</span>
		{#if owner}
			<button class="ml-auto btn btn-sm" onclick={open}>审核并导入</button>
		{/if}
	</div>
{/if}

{#if importing}
	<Modal title="从服务器导入{noun}" wide onclose={() => (importing = false)}>
		<p class="mb-3 text-[13px] text-mist-400">
			这些{noun}已存在于下列服务器，但尚未加入“{org.name}”的组织列表。导入后会标记为由面板管理，并同步到组织内其他服务器。之后删除导入条目时，面板会从受管理的服务器上移除对应条目。
		</p>
		{#if mine.length > 5}
			<input
				class="mb-3 input w-full sm:w-72"
				type="search"
				placeholder="按名称、SteamID 或服务器筛选…"
				aria-label="筛选候选人"
				bind:value={search}
			/>
		{/if}
		<div class="max-h-[50vh] table-wrap overflow-y-auto">
			<table>
				<thead>
					<tr>
						<th></th>
						<th>玩家</th>
						<th>所在服务器</th>
						{#if kind === 'ban'}<th>原因</th>{/if}
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
												<span class="text-mist-600">执行人：{s.bannedBy}</span>{/if}
										</div>
									{/each}
								</td>
							{/if}
						</tr>
					{:else}
						<tr><td colspan="4" class="py-6 text-center text-mist-600">没有符合条件的玩家。</td></tr
						>
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
				}}>{shown.every((c) => picked[c.steamId]) ? '取消全选' : '全选'}</button
			>
			<button type="button" class="btn" data-close onclick={() => (importing = false)}>取消</button>
			<button type="button" class="btn btn-primary" disabled={busy} onclick={run}
				>导入 {mine.filter((c) => picked[c.steamId]).length}</button
			>
		</div>
	</Modal>
{/if}
