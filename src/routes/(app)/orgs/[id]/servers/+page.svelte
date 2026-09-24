<script lang="ts">
	// The organisation's servers as this user may open them, with the worker's live view of each.
	// Adding, editing and sharing a server happens on the Servers page; this tab is where an org
	// owner sees them side by side.
	import { watchLive } from '$lib/live';
	import { fmtNum, mapName } from '$lib/format';
	import Badge from '$lib/components/Badge.svelte';
	import Pulse from '$lib/components/Pulse.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { LiveView } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let owner = $derived(data.listsRole.owner);
	let live = $state<Record<string, LiveView>>({});
	$effect(() => {
		const ids = data.orgServers.map((s) => s.id);
		return watchLive(ids, (v) => {
			live[v.serverId] = v;
		});
	});

	let q = $state('');
	type Row = (typeof data.orgServers)[number];
	const status = (s: Row) => (live[s.id]?.ok ? live[s.id].status : null);
	const sort = new TableSort<Row>({
		server: { by: (s) => s.name },
		target: { by: (s) => `${s.host}:${s.port}` },
		map: { by: (s) => (status(s) ? mapName(status(s)!.map) : null) },
		players: { by: (s) => status(s)?.playerCount, dir: 'desc' },
		access: { by: (s) => s.roleName }
	});
	let shown = $derived(
		sort.sorted(
			data.orgServers.filter((s) => {
				const st = live[s.id]?.status;
				return matches(
					q,
					s.name,
					`${s.host}:${s.port}`,
					s.notes,
					st?.serverName,
					st ? mapName(st.map) : null
				);
			})
		)
	);
	let seen = $derived(data.orgServers.filter((s) => live[s.id]));
	let reachable = $derived(seen.filter((s) => live[s.id].ok).length);
	let playing = $derived(
		seen.reduce((n, s) => n + (live[s.id].ok ? (live[s.id].status?.playerCount ?? 0) : 0), 0)
	);
</script>

<div class="mb-4 flex flex-wrap items-center gap-3">
	<div>
		<h2 class="text-lg font-semibold tracking-tight">
			服务器
			<span class="font-normal text-mist-600"
				>{data.orgServers.length} / {data.org.serverLimit}</span
			>
		</h2>
		<p class="text-[13px] text-mist-400">
			{#if !data.orgServers.length}
				这里还没有内容。
			{:else if seen.length}
				{reachable} of {data.orgServers.length} 可连接， {fmtNum(playing)} 当前在线。
			{:else}
				检查中…
			{/if}
		</p>
	</div>
	{#if owner}
		<span class="ml-auto inline-flex gap-1.5">
			<a class="btn btn-primary" href="/servers">管理服务器</a>
		</span>
	{/if}
</div>

{#if !data.orgServers.length}
	<div class="callout">
		{#if owner}
			还没有服务器。 <a href="/servers" class="font-semibold text-accent underline">添加</a> 上的“服务器”页面添加；通过邀请链接获得默认服务器角色的成员，只会访问其加入时已经存在的服务器。
		{:else}
			你无权访问其中任何一台 {data.org.name}的服务器。
		{/if}
	</div>
{:else}
	{#if data.orgServers.length > 5}
		<div class="mb-3">
			<input
				class="input sm:w-72"
				type="search"
				placeholder="搜索名称、主机或地图…"
				bind:value={q}
				aria-label="搜索服务器"
			/>
		</div>
	{/if}
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<SortHeader {sort} key="server">服务器</SortHeader>
					<SortHeader {sort} key="target">目标</SortHeader>
					<SortHeader {sort} key="map">地图</SortHeader>
					<SortHeader {sort} key="players" num>玩家</SortHeader>
					<SortHeader {sort} key="access">你的权限</SortHeader>
					<th></th>
				</tr>
			</thead>
			<tbody>
				{#each shown as s (s.id)}
					{@const v = live[s.id]}
					{@const st = v?.ok ? v.status : null}
					<tr>
						<td>
							<span class="inline-flex min-w-0 items-center gap-2">
								<Pulse ok={v ? v.ok : undefined} />
								<a
									href="/server/{encodeURIComponent(s.id)}"
									class="truncate font-medium text-accent hover:underline">{s.name}</a
								>
								{#if s.demo}<Badge tone="info">演示</Badge>{/if}
							</span>
							{#if st?.serverName && st.serverName !== s.name}
								<div class="truncate text-[12px] text-mist-400">{st.serverName}</div>
							{/if}
							{#if s.notes}<div class="text-[12px] text-mist-400">{s.notes}</div>{/if}
						</td>
						<td class="font-mono text-[12.5px] text-mist-400">{s.host}:{s.port}</td>
						<td>
							{#if st}
								{mapName(st.map)}
								{#if st.matchSeconds !== null}<div class="text-[12px] text-mist-400">
										{Math.floor(st.matchSeconds / 60)} 分钟内
									</div>{/if}
							{:else if v}
								<span class="text-[12.5px] text-danger">{v.error || 'Unreachable.'}</span>
							{:else}
								<span class="text-mist-600">—</span>
							{/if}
						</td>
						<td class="num whitespace-nowrap">
							{#if st}
								<b>{fmtNum(st.playerCount)}</b>
								<span class="text-mist-400">/ {fmtNum(st.maxPlayers)}</span>{#if v.reservedSlots}
									<div class="text-[12px] text-mist-400">+ {v.reservedSlots} 预留</div>{/if}
							{:else}
								<span class="text-mist-600">—</span>
							{/if}
						</td>
						<td><RoleBadge role={s.roleName} /></td>
						<td class="text-right whitespace-nowrap">
							<span class="inline-flex gap-1.5">
								<a class="btn btn-sm" href="/server/{encodeURIComponent(s.id)}/players">玩家</a>
								<a class="btn btn-sm" href="/server/{encodeURIComponent(s.id)}/slots">预留位</a>
							</span>
						</td>
					</tr>
				{:else}
					<tr><td colspan="6" class="py-8 text-center text-mist-600">没有符合条件的服务器。</td></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
{/if}
