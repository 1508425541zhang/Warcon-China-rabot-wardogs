<script lang="ts">
	import { goto, invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { OrgView } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let siteOwner = $derived(data.user.role === 'owner');

	let search = $state('');
	const sort = new TableSort<(typeof data.orgViews)[number]>({
		name: { by: (o) => o.name },
		created: { by: (o) => o.createdAt, dir: 'desc' },
		status: { by: (o) => !!o.suspended },
		role: { by: (o) => o.role },
		members: { by: (o) => o.memberCount, dir: 'desc' },
		servers: { by: (o) => o.serverCount, dir: 'desc' }
	});
	let rows = $derived(
		sort.sorted(data.orgViews.filter((o) => matches(search, o.name, o.slug, o.createdBy?.username)))
	);

	type Dialog =
		{ kind: 'create'; name: string } | { kind: 'suspend'; org: OrgView; reason: string };
	let dialog = $state<Dialog | null>(null);
	let busy = $state(false);

	async function run(fn: () => Promise<void>, done: string) {
		busy = true;
		try {
			await fn();
			toast(done, 'ok');
			dialog = null;
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function create() {
		const d = dialog;
		if (!d || d.kind !== 'create') return;
		busy = true;
		try {
			const res = await api<{ id: string }>('POST', '/api/orgs', { name: d.name.trim() });
			toast('组织已创建。', 'ok');
			dialog = null;
			await invalidateAll();
			await goto(`/orgs/${encodeURIComponent(res.id)}`);
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	function suspend() {
		const d = dialog;
		if (!d || d.kind !== 'suspend') return;
		void run(
			() =>
				api('PATCH', `/api/orgs/${encodeURIComponent(d.org.id)}`, {
					suspended: true,
					reason: d.reason.trim()
				}),
			`${d.org.name} suspended.`
		);
	}
	function restore(o: OrgView) {
		void run(
			() => api('PATCH', `/api/orgs/${encodeURIComponent(o.id)}`, { suspended: false }),
			`${o.name} 已恢复。`
		);
	}
	async function remove(o: OrgView) {
		if (
			!(await confirmDialog(
				`确定删除“${o.name}”？这会从面板移除其 ${o.serverCount} 台服务器、所有成员关系和邀请链接。审计历史会保留。`,
				{ okLabel: '删除组织', danger: true }
			))
		)
			return;
		await run(() => api('DELETE', `/api/orgs/${encodeURIComponent(o.id)}`), `${o.name} 已删除。`);
	}
</script>

<svelte:head><title>组织 · {data.appName}</title></svelte:head>

<div class="mb-5 flex items-center gap-3">
	<h1 class="text-xl font-semibold tracking-tight">组织</h1>
	{#if data.canCreateOrg}
		<button class="ml-auto btn btn-primary" onclick={() => (dialog = { kind: 'create', name: '' })}
			>新组织</button
		>
	{/if}
</div>

<div class="callout">
	An <b>组织</b> 是拥有独立服务器、成员和邀请链接的战队或社区。其
	<b>所有者</b> 可以添加服务器、创建邀请链接，并决定各服务器的成员角色；他们管理组织内所有服务器。
	<b>成员</b>
	仅能查看获授权的服务器。
	{#if siteOwner}作为平台所有者，你可以管理所有组织：在此或组织页面提高服务器上限、暂停或删除组织。{/if}
</div>

{#if data.orgViews.length > 5}
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<input
			class="input w-full sm:w-72"
			type="search"
			placeholder="搜索名称或标识…"
			aria-label="搜索组织"
			bind:value={search}
		/>
		{#if search.trim()}
			<span class="text-[12.5px] text-mist-400">{rows.length} of {data.orgViews.length}</span>
		{/if}
	</div>
{/if}

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<SortHeader {sort} key="name">组织</SortHeader>
				{#if siteOwner}
					<SortHeader {sort} key="created">创建时间</SortHeader>
					<SortHeader {sort} key="status">状态</SortHeader>
				{:else}
					<SortHeader {sort} key="role">你的角色</SortHeader>
				{/if}
				<SortHeader {sort} key="members" num>成员</SortHeader>
				<SortHeader {sort} key="servers" num>服务器</SortHeader>
				<th></th>
			</tr>
		</thead>
		<tbody>
			{#each rows as o (o.id)}
				<tr class={o.suspended ? 'text-mist-400' : ''}>
					<td>
						{#if o.role === 'owner' && (!o.suspended || siteOwner)}
							<a
								href="/orgs/{encodeURIComponent(o.id)}"
								class="font-medium text-accent hover:underline">{o.name}</a
							>
						{:else if o.listKinds.length}
							<a
								href="/orgs/{encodeURIComponent(o.id)}/{o.listKinds.includes('ban')
									? 'bans'
									: 'reserved'}"
								class="font-medium text-accent hover:underline">{o.name}</a
							>
						{:else}
							<span class="font-medium">{o.name}</span>
						{/if}
						<div class="font-mono text-[12px] text-mist-600">{o.slug}</div>
					</td>
					{#if siteOwner}
						<td class="whitespace-nowrap">
							<div>{fmtTime(o.createdAt)}</div>
							<div class="font-mono text-[12px] text-mist-600">
								{o.createdBy ? `@${o.createdBy.username}` : '—'}
							</div>
						</td>
						<td>
							{#if o.suspended}
								<Badge tone="err">已停用</Badge>
								{#if o.suspended.reason}<div
										class="mt-1 max-w-[220px] truncate text-[12px] text-mist-600"
										title={o.suspended.reason}
									>
										{o.suspended.reason}
									</div>{/if}
							{:else}
								<Badge tone="ok">启用中</Badge>
							{/if}
							{#if o.allowPublicStatus || o.allowPublicLeaderboards}
								<div class="mt-1 text-[12px] text-mist-600">
									公开： {[
										o.allowPublicStatus ? 'status' : '',
										o.allowPublicLeaderboards ? 'leaderboards' : ''
									]
										.filter(Boolean)
										.join(', ')}
								</div>
							{/if}
						</td>
					{:else}
						<td>
							<RoleBadge role={o.role} />
							{#if o.suspended}<Badge tone="err" class="ml-1">已停用</Badge>{/if}
						</td>
					{/if}
					<td class="num">{o.memberCount}</td>
					<td class="num">
						{o.serverCount}
						<span class="text-mist-600">/ {o.serverLimit}</span>
					</td>
					<td class="text-right whitespace-nowrap">
						<span class="inline-flex gap-1.5">
							{#if o.role === 'owner' && (!o.suspended || siteOwner)}
								<a class="btn btn-sm" href="/orgs/{encodeURIComponent(o.id)}">管理</a>
							{/if}
							{#if o.listKinds.includes('ban')}
								<a class="btn btn-sm" href="/orgs/{encodeURIComponent(o.id)}/bans">封禁名单</a>
							{/if}
							{#if o.listKinds.includes('reserve')}
								<a class="btn btn-sm" href="/orgs/{encodeURIComponent(o.id)}/reserved">预留位</a>
							{/if}
							{#if siteOwner}
								{#if o.suspended}
									<button class="btn btn-sm" onclick={() => restore(o)} disabled={busy}>恢复</button
									>
								{:else}
									<button
										class="btn btn-sm"
										onclick={() => (dialog = { kind: 'suspend', org: o, reason: '' })}
										disabled={busy}>停用</button
									>
								{/if}
								<button class="btn btn-sm btn-danger" onclick={() => remove(o)} disabled={busy}
									>删除</button
								>
							{/if}
						</span>
					</td>
				</tr>
			{:else}
				<tr
					><td colspan="6" class="py-8 text-center text-mist-600"
						>{siteOwner
							? '还没有组织。'
							: data.canCreateOrg
								? '你尚未加入组织。可以索取邀请链接，或创建自己的组织。'
								: '你尚未加入组织，请索取邀请链接。'}</td
					></tr
				>
			{/each}
		</tbody>
	</table>
</div>

{#if dialog?.kind === 'create'}
	{@const d = dialog}
	<Modal title="新组织" onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				void create();
			}}
		>
			<label class="block"
				><span class="field-label">名称</span><input
					class="input"
					type="text"
					bind:value={d.name}
					placeholder="战队或社区名称"
					minlength="2"
					maxlength="60"
					required
				/></label
			>
			<p class="note">你将成为首位所有者。创建后可在组织页面添加服务器和邀请链接。</p>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}>创建</button>
			</div>
		</form>
	</Modal>
{:else if dialog?.kind === 'suspend'}
	{@const d = dialog}
	<Modal title="停用 {d.org.name}" onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				suspend();
			}}
		>
			<p class="text-[13.5px]">
				成员将失去服务器访问权限，组织所有者无法添加服务器或创建邀请链接，邀请链接也会失效，直到恢复组织。数据不会删除。
			</p>
			<label class="block"
				><span class="field-label">原因（组织所有者可见）</span><input
					class="input"
					type="text"
					bind:value={d.reason}
					maxlength="300"
				/></label
			>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button type="submit" class="btn btn-danger" disabled={busy}>停用</button>
			</div>
		</form>
	</Modal>
{/if}
