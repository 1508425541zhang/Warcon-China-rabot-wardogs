<script lang="ts">
	// Everyone this organisation has seen on the servers you can open: every name they used, when
	// and how much they played, and the ban, reserve and watch actions for someone who is not online.
	import { goto, invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { api, qs, errorMessage } from '$lib/api';
	import { can } from '$lib/capabilities';
	import { fmtDuration, fmtNum, fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import BanDialog from '$lib/components/BanDialog.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import type { SortLike } from '$lib/table.svelte';
	import type { SeenPlayer, SeenSort } from '$lib/server/seen';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let orgId = $derived(data.org.id);
	/** each row action goes on one org list, and is offered to that list's editors */
	let canBan = $derived(data.listsRole.kinds.includes('ban'));
	let canReserve = $derived(data.listsRole.kinds.includes('reserve'));
	/**
	 * The watchlist is the organisation's, so Notes on any of its servers may mark a player: the
	 * mark goes through the player's last server where the caller keeps notes there, else through
	 * one where they do.
	 */
	let notesOn = $derived(
		data.orgServers.filter((s) => can(s.caps, 'players.notes')).map((s) => s.id)
	);
	const watchVia = (p: SeenPlayer): string | null =>
		notesOn.includes(p.lastServerId) ? p.lastServerId : (notesOn[0] ?? null);
	let apiBase = $derived(`/api/orgs/${encodeURIComponent(orgId)}/players`);

	let extra = $state<SeenPlayer[]>([]);
	let loadingMore = $state(false);
	$effect(() => {
		void data.players;
		extra = [];
	});
	let rows = $derived([...data.players, ...extra]);
	let more = $derived(rows.length < data.total);

	let f = $state({ q: '', server: '', since: '', flag: '' });
	$effect(() => {
		const d = data.filters;
		f = {
			q: d.q,
			server: d.serverId,
			since: d.since ? String(d.since) : '',
			flag: d.flag
		};
	});
	const query = (over: Partial<{ sort: SeenSort; dir: 'asc' | 'desc' }> = {}) => ({
		q: f.q.trim(),
		server: f.server,
		since: f.since,
		flag: f.flag,
		sort: over.sort ?? data.filters.sort,
		dir: over.dir ?? data.filters.dir
	});
	let timer: ReturnType<typeof setTimeout> | undefined;
	function apply(over: Partial<{ sort: SeenSort; dir: 'asc' | 'desc' }> = {}) {
		void goto(`${page.url.pathname}${qs(query(over))}`, {
			keepFocus: true,
			noScroll: true,
			replaceState: true
		});
	}
	function applyDebounced() {
		clearTimeout(timer);
		timer = setTimeout(() => apply(), 300);
	}
	/** the server does the ordering here: a header click becomes sort/dir params */
	const sort: SortLike<SeenSort> = {
		get key() {
			return data.filters.sort;
		},
		get dir() {
			return data.filters.dir;
		},
		toggle(key) {
			const same = data.filters.sort === key;
			const dir = same
				? data.filters.dir === 'asc'
					? 'desc'
					: 'asc'
				: key === 'name'
					? 'asc'
					: 'desc';
			apply({ sort: key, dir });
		}
	};

	async function loadMore() {
		if (loadingMore || !more) return;
		loadingMore = true;
		try {
			const d = await api<{ players: SeenPlayer[] }>(
				'GET',
				`${apiBase}${qs({ ...query(), offset: rows.length, limit: data.pageSize })}`
			);
			extra = [...extra, ...d.players];
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			loadingMore = false;
		}
	}

	// ---- row actions ----
	let banning = $state<SeenPlayer | null>(null);
	let busy = $state('');
	async function reserve(p: SeenPlayer) {
		if (
			!(await confirmDialog(
				`确定为 ${p.name}（${p.steamId}）分配“${data.org.name}”组织所有服务器的预留席位？`,
				{
					okLabel: '分配席位'
				}
			))
		)
			return;
		busy = p.steamId;
		try {
			await api('POST', `/api/orgs/${encodeURIComponent(orgId)}/lists/reserve/entries`, {
				steamId: p.steamId,
				reason: ''
			});
			toast(`${p.name} 已加入组织预留席位列表。`, 'ok');
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = '';
		}
	}
	async function watch(p: SeenPlayer) {
		const via = watchVia(p);
		if (!via) return;
		busy = p.steamId;
		try {
			await api('PUT', `/api/servers/${encodeURIComponent(via)}/players/${p.steamId}/watch`, {
				watched: !p.watched,
				reason: ''
			});
			toast(p.watched ? `${p.name} 已移出关注名单。` : `${p.name} 已加入关注名单。`, 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = '';
		}
	}
	const kd = (p: SeenPlayer) => (p.deaths ? (p.kills / p.deaths).toFixed(2) : p.kills ? '∞' : '—');
	const dossier = (p: SeenPlayer) =>
		`/server/${encodeURIComponent(p.lastServerId)}/players/${p.steamId}`;
</script>

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0!">出现过的玩家</span>
		<span class="text-[12.5px] text-mist-600"
			>{fmtNum(data.total)} 位于你可访问的服务器上 · 昵称、游玩时间和统计数据仅保留在 {data.org
				.name}</span
		>
	</div>
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<input
			class="input w-full sm:w-72"
			type="search"
			placeholder="名称、曾用名或 SteamID…"
			aria-label="搜索玩家"
			bind:value={f.q}
			oninput={applyDebounced}
		/>
		<select
			class="input w-full sm:w-52"
			aria-label="服务器"
			bind:value={f.server}
			onchange={() => apply()}
		>
			<option value="">任意服务器</option>
			{#each data.orgServers as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
		</select>
		<select
			class="input w-full sm:w-40"
			aria-label="出现时间范围"
			bind:value={f.since}
			onchange={() => apply()}
		>
			<option value="">全部时间</option>
			<option value="1">最近 24 小时</option>
			<option value="7">最近 7 天</option>
			<option value="30">最近 30 天</option>
			<option value="90">最近 90 天</option>
		</select>
		<select
			class="input w-full sm:w-40"
			aria-label="标记"
			bind:value={f.flag}
			onchange={() => apply()}
		>
			<option value="">所有人</option>
			<option value="online">当前在线</option>
			<option value="banned">已封禁</option>
			<option value="watched">在观察名单中</option>
		</select>
	</div>
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<SortHeader {sort} key="name">玩家</SortHeader>
					<SortHeader {sort} key="firstSeen">首次出现</SortHeader>
					<SortHeader {sort} key="lastSeen">最近出现</SortHeader>
					<SortHeader {sort} key="sessions" num>场次</SortHeader>
					<SortHeader {sort} key="minutes" num>游戏时间</SortHeader>
					<SortHeader {sort} key="kills" num>K</SortHeader>
					<SortHeader {sort} key="deaths" num>D</SortHeader>
					<th class="num">K/D</th>
					<th>服务器</th>
					<th></th>
				</tr>
			</thead>
			<tbody>
				{#each rows as p (p.steamId)}
					<tr>
						<td class="max-w-[320px]">
							<div class="flex items-start gap-2.5">
								{#if p.steam?.avatar}<img
										src={p.steam.avatar}
										alt=""
										class="mt-0.5 size-6 shrink-0 rounded-sm"
										loading="lazy"
										referrerpolicy="no-referrer"
									/>{/if}
								<div class="min-w-0">
									<div class="flex flex-wrap items-center gap-1.5">
										<a
											href={dossier(p)}
											class="truncate font-medium hover:text-accent hover:underline"
											title="打开玩家档案">{p.name}</a
										>
										{#if p.online}<span class="size-1.5 rounded-full bg-ok" title="当前在线"
											></span>{/if}
										{#if p.banned}<Badge tone="err"
												>{p.banned === 'org' ? 'banned' : '本服已封禁'}</Badge
											>{/if}
										{#if p.watched}<Badge tone="warn">观察</Badge>{/if}
									</div>
									{#if p.aliases.length}
										<div class="truncate text-[12px] text-mist-400" title={p.aliases.join(', ')}>
											以及 {p.aliases.slice(0, 4).join(', ')}{#if p.aliases.length > 4}
												和 {p.aliases.length - 4} 更多{/if}
										</div>
									{/if}
									<div class="font-mono text-[11.5px] text-mist-600">
										{p.steamId}{#if p.steam?.persona && p.steam.persona !== p.name}
											· Steam: {p.steam.persona}{/if}
									</div>
								</div>
							</div>
						</td>
						<td class="whitespace-nowrap text-mist-400">{fmtTime(p.firstSeen)}</td>
						<td class="whitespace-nowrap">{fmtTime(p.lastSeen)}</td>
						<td class="num">{fmtNum(p.sessions)}</td>
						<td class="num whitespace-nowrap">{fmtDuration(p.minutes * 60)}</td>
						<td class="num">{fmtNum(p.kills)}</td>
						<td class="num">{fmtNum(p.deaths)}</td>
						<td class="num">{kd(p)}</td>
						<td class="whitespace-nowrap text-mist-400"
							>{p.servers > 1 ? `${p.servers} · last ` : ''}{p.lastServerName || '—'}</td
						>
						<td class="py-1.5 text-right whitespace-nowrap">
							<div class="inline-flex gap-1.5">
								{#if watchVia(p) || canReserve}
									<div class="join">
										{#if watchVia(p)}
											<button
												class="btn btn-sm"
												disabled={busy === p.steamId}
												onclick={() => watch(p)}>{p.watched ? 'Unwatch' : 'Watch'}</button
											>
										{/if}
										{#if canReserve}
											<button
												class="btn btn-sm"
												disabled={busy === p.steamId}
												onclick={() => reserve(p)}>预留</button
											>
										{/if}
									</div>
								{/if}
								{#if canBan}
									<button
										class="btn btn-sm btn-danger"
										disabled={busy === p.steamId || p.banned === 'org'}
										title={p.banned === 'org' ? '已在组织封禁列表中' : ''}
										onclick={() => (banning = p)}>封禁</button
									>
								{/if}
							</div>
						</td>
					</tr>
				{:else}
					<tr
						><td colspan="10" class="py-6 text-center text-mist-600"
							>{data.filters.q || data.filters.serverId || data.filters.since || data.filters.flag
								? '没有符合条件的玩家。'
								: '尚未观测到玩家。玩家加入工作进程监控的服务器后会显示在这里。'}</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
	<div class="mt-3 flex flex-wrap items-center gap-3">
		<span class="text-[12.5px] text-mist-600">{rows.length} of {fmtNum(data.total)}</span>
		{#if more}
			<button class="btn btn-sm" disabled={loadingMore} onclick={loadMore}
				>{loadingMore ? 'Loading…' : '加载更多'}</button
			>
		{/if}
	</div>
	<p class="note">
		根据工作进程在你可访问服务器上记录的会话生成，因此只显示曾加入这些服务器的玩家及其在这些服务器使用的昵称。游玩时间为会话时长之和。{#if canBan}
			封禁会加入组织封禁列表。{/if}{#if canReserve}
			预留席位会加入组织预留列表。{/if}{#if notesOn.length}
			关注会在整个组织内标记该玩家。{/if}
	</p>
</div>

{#if banning}
	{#key banning.steamId}
		<BanDialog
			{orgId}
			orgName={data.org.name}
			steamId={banning.steamId}
			name={banning.name}
			canOrg={canBan}
			onclose={() => (banning = null)}
			ondone={() => invalidateAll()}
		/>
	{/key}
{/if}
