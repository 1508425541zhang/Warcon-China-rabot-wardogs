<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import RoleSelect from '$lib/components/RoleSelect.svelte';
	import type { OrgMemberView } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let orgPath = $derived(`/api/orgs/${encodeURIComponent(data.org.id)}`);

	type Grid = Record<string, Record<string, string>>;
	/** what the server holds: member -> server -> role id ('' for none) */
	let saved = $derived.by<Grid>(() => {
		const g: Grid = {};
		for (const m of data.members) {
			g[m.userId] = {};
			for (const s of data.orgServers)
				g[m.userId][s.id] = m.grants.find((x) => x.serverId === s.id)?.roleId ?? '';
		}
		return g;
	});
	/** what is being edited; rebuilt from the saved state whenever that changes */
	let grid = $state<Grid>({});
	$effect(() => {
		grid = structuredClone($state.snapshot(saved));
	});

	// Owners (and the site owner) hold everything everywhere: no cells to edit.
	const fixed = (m: OrgMemberView) => m.role === 'owner' || m.siteOwner;
	const editable = $derived(data.members.filter((m) => !fixed(m)));

	let q = $state('');
	let shown = $derived.by(() => {
		const needle = q.trim().toLowerCase();
		if (!needle) return data.members;
		return data.members.filter((m) => `${m.name} ${m.username}`.toLowerCase().includes(needle));
	});
	let shownEditable = $derived(shown.filter((m) => !fixed(m)));

	const rowOf = (userId: string) => grid[userId] ?? {};
	const changed = (userId: string) =>
		data.orgServers.some((s) => (rowOf(userId)[s.id] ?? '') !== (saved[userId]?.[s.id] ?? ''));
	let dirty = $derived(editable.filter((m) => changed(m.userId)));

	/** the role id shared by every shown member on this server, or null when mixed */
	const columnCommon = (serverId: string): string | null => {
		if (!shownEditable.length) return null;
		const first = rowOf(shownEditable[0].userId)[serverId] ?? '';
		return shownEditable.every((m) => (rowOf(m.userId)[serverId] ?? '') === first) ? first : null;
	};
	const rowCommon = (userId: string): string | null => {
		if (!data.orgServers.length) return null;
		const first = rowOf(userId)[data.orgServers[0].id] ?? '';
		return data.orgServers.every((s) => (rowOf(userId)[s.id] ?? '') === first) ? first : null;
	};
	function setColumn(serverId: string, role: string) {
		for (const m of shownEditable) grid[m.userId][serverId] = role;
	}
	function setRow(userId: string, role: string) {
		for (const s of data.orgServers) grid[userId][s.id] = role;
	}

	let busy = $state(false);
	async function save() {
		busy = true;
		const todo = dirty;
		let done = 0;
		try {
			for (const m of todo) {
				const grants = data.orgServers
					.filter((s) => grid[m.userId][s.id])
					.map((s) => ({ serverId: s.id, roleId: grid[m.userId][s.id] }));
				await api('PUT', `${orgPath}/members/${m.userId}/grants`, { grants });
				done++;
			}
			toast(`已更新 ${done} 位成员的访问权限。`, 'ok');
		} catch (err) {
			toast(`${errorMessage(err)}${done ? `（已保存 ${done} / ${todo.length} 位）` : ''}`, 'err');
		} finally {
			busy = false;
			await invalidateAll();
		}
	}
	function discard() {
		grid = structuredClone($state.snapshot(saved));
	}
</script>

<div class="mb-4 flex flex-wrap items-center gap-3">
	<div class="text-[13.5px] text-mist-400">
		每个单元格表示一位成员在一台服务器上的角色。组织所有者可以管理所有服务器，因此不列在表格中。角色权限可在
		<a href="/orgs/{encodeURIComponent(data.org.id)}/roles" class="text-accent underline">角色</a> 选项卡。
	</div>
	<div class="ml-auto flex items-center gap-2">
		{#if data.members.length > 8}
			<input
				class="input w-48"
				type="search"
				placeholder="筛选成员…"
				bind:value={q}
				aria-label="筛选成员"
			/>
		{/if}
	</div>
</div>

{#if !data.orgServers.length}
	<div class="callout">
		这个组织还没有服务器。 <a href="/servers" class="font-semibold text-accent underline">添加</a> 添加后再回来分配访问权限。
	</div>
{:else if !data.members.length}
	<div class="callout">
		还没有成员。请在 <a
			href="/orgs/{encodeURIComponent(data.org.id)}"
			class="font-semibold text-accent underline">总览</a
		>.
	</div>
{:else}
	<div class="access-grid table-wrap">
		<table>
			<thead>
				<tr>
					<th class="sticky-col">成员</th>
					{#each data.orgServers as s (s.id)}
						<th class="text-center">
							<div
								class="truncate tracking-normal normal-case"
								title="{s.name} · {s.host}:{s.port}"
							>
								{s.name}
							</div>
							<div class="mt-1.5">
								<RoleSelect
									value={columnCommon(s.id)}
									roles={data.roles}
									mixed
									label="{s.name} 上的所有成员"
									onchange={(v: string) => setColumn(s.id, v)}
									disabled={busy || !shownEditable.length}
								/>
							</div>
						</th>
					{/each}
					<th class="text-center">每台服务器</th>
				</tr>
			</thead>
			<tbody>
				{#each shown as m (m.userId)}
					{@const isFixed = fixed(m)}
					<tr class={changed(m.userId) ? 'changed' : ''}>
						<td class="sticky-col">
							<div class="flex items-center gap-1.5">
								<span class="truncate">{m.name || m.username}</span>
								{#if m.siteOwner}<Badge tone="accent">站点所有者</Badge
									>{:else if m.role === 'owner'}<Badge tone="accent">所有者</Badge>{/if}
								{#if m.disabled}<Badge tone="err">已停用</Badge>{/if}
							</div>
							<div class="font-mono text-[12px] text-mist-600">@{m.username}</div>
						</td>
						{#if isFixed}
							<td colspan={data.orgServers.length + 1} class="text-mist-600">
								对每台服务器拥有全部权限{m.siteOwner ? '（平台所有者）' : '（组织所有者）'}
							</td>
						{:else}
							{#each data.orgServers as s (s.id)}
								<td class="text-center">
									{#if grid[m.userId]}
										<RoleSelect
											bind:value={grid[m.userId][s.id]}
											roles={data.roles}
											label="{m.username} 在 {s.name} 上的角色"
											disabled={busy}
										/>
									{/if}
								</td>
							{/each}
							<td class="text-center">
								<RoleSelect
									value={rowCommon(m.userId)}
									roles={data.roles}
									mixed
									label="{m.username} on every server"
									onchange={(v: string) => setRow(m.userId, v)}
									disabled={busy}
								/>
							</td>
						{/if}
					</tr>
				{:else}
					<tr
						><td colspan={data.orgServers.length + 2} class="py-6 text-center text-mist-600"
							>没有符合条件的成员。</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>

	{#if dirty.length}
		<div
			class="sticky bottom-3 z-10 mt-4 flex flex-wrap items-center gap-3 rounded-card border border-accent/50 bg-ink-900 px-4 py-3 shadow-pop"
		>
			<span class="text-[13.5px]"><b>{dirty.length}</b> 位成员有未保存的修改</span>
			<span class="ml-auto inline-flex gap-2">
				<button class="btn" onclick={discard} disabled={busy}>放弃</button>
				<button class="btn btn-primary" onclick={save} disabled={busy}>保存权限</button>
			</span>
		</div>
	{/if}
{/if}

<style>
	/* First column stays put while the server columns scroll sideways. */
	.access-grid :global(.sticky-col) {
		position: sticky;
		left: 0;
		z-index: 2;
		max-width: 220px;
		background: var(--color-ink-950);
		box-shadow: inset -1px 0 0 rgb(0 0 0);
	}
	.access-grid :global(th.sticky-col) {
		z-index: 3;
		background: var(--color-ink-900);
	}
	.access-grid :global(tbody tr:hover td.sticky-col) {
		background: var(--color-ink-900);
	}
	.access-grid :global(tr.changed td:first-child) {
		box-shadow:
			inset 2px 0 0 var(--color-accent),
			inset -1px 0 0 rgb(0 0 0);
	}
	.access-grid :global(th) {
		vertical-align: bottom;
		max-width: 160px;
	}
</style>
