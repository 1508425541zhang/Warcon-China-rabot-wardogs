<script lang="ts">
	// The leaderboard as both the panel tab and the public page show it: the controls (scope,
	// range, playtime floor), one page of ranked rows with sortable headers, and the pager. The
	// board itself comes from the caller, which reloads it whenever `onchange` hands back a query.
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { fmtCash } from '$lib/cash';
	import { fmtMinutes, fmtNum, fmtTime, fmtAgo } from '$lib/format';
	import {
		BOARD_RANGES,
		kdRatio,
		perHour,
		winRate,
		type BoardMetric,
		type BoardQuery,
		type BoardView
	} from '$lib/leaderboard';
	import type { SortLike } from '$lib/table.svelte';

	let {
		board,
		query,
		loading = false,
		onchange,
		hrefFor,
		orgName = '',
		/** offer the organisation scope (the panel always does; a public page only with more than one public server) */
		orgScope = true,
		/** show SteamIDs under the names (the panel does, a public page does not) */
		showIds = false,
		/** relative "last seen" times (public pages) rather than clock times */
		relative = false
	}: {
		board: BoardView | null;
		query: BoardQuery;
		loading?: boolean;
		onchange: (q: BoardQuery) => void;
		hrefFor: (steamId: string) => string;
		orgName?: string;
		orgScope?: boolean;
		showIds?: boolean;
		relative?: boolean;
	} = $props();

	const set = (patch: Partial<BoardQuery>) =>
		onchange({ ...query, ...patch, page: patch.page ?? 1 });
	// Every metric ranks biggest first on the first click, then the other way round; there is
	// no "off" since a board is always in some order.
	const sort: SortLike<BoardMetric> = {
		get key() {
			return query.sort;
		},
		get dir() {
			return query.dir;
		},
		toggle(key: BoardMetric) {
			if (query.sort !== key) set({ sort: key, dir: 'desc' });
			else set({ dir: query.dir === 'desc' ? 'asc' : 'desc' });
		}
	};
	// The field holds what is being typed; it follows the query whenever that changes.
	let floor = $state('');
	$effect(() => {
		floor = String(query.minMinutes);
	});
	function applyFloor() {
		const n = Math.max(0, Math.round(Number(floor) || 0));
		if (n !== query.minMinutes) set({ minMinutes: n });
	}
	let pages = $derived(
		board
			? Math.min(board.maxPage ?? Infinity, Math.max(1, Math.ceil(board.total / board.pageSize)))
			: 1
	);
	const ratio = (v: number | null, digits = 2) => (v === null ? '—' : v.toFixed(digits));
	const pct = (v: number | null) => (v === null ? '—' : `${Math.round(v * 100)}%`);
	const seen = (iso: string | null) => (iso ? (relative ? fmtAgo(iso) : fmtTime(iso)) : '—');
</script>

<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
	{#if orgScope}
		<div class="join">
			<button
				class="btn btn-sm {query.scope === 'server' ? 'btn-primary' : ''}"
				onclick={() => set({ scope: 'server' })}>本服务器</button
			>
			<button
				class="btn btn-sm {query.scope === 'org' ? 'btn-primary' : ''}"
				onclick={() => set({ scope: 'org' })}
				title={orgName ? `Every server of ${orgName}` : '组织中的每台服务器'}>组织</button
			>
		</div>
	{/if}
	<div class="join">
		{#each BOARD_RANGES as r (r.key)}
			<button
				class="btn btn-sm {query.range === r.key ? 'btn-primary' : ''}"
				onclick={() => set({ range: r.key })}>{r.label}</button
			>
		{/each}
	</div>
	<label class="join items-center" title="低于游戏时间门槛的玩家不会纳入统计">
		<span class="pointer-events-none btn btn-sm">至少</span>
		<input
			class="h-[30px] input w-20 py-0 text-right"
			type="number"
			min="0"
			step="10"
			aria-label="最低游戏时间（分钟）"
			bind:value={floor}
			onchange={applyFloor}
			onkeydown={(e) => e.key === 'Enter' && applyFloor()}
		/>
		<span class="pointer-events-none btn btn-sm">最短游戏时间</span>
	</label>
	<span class="ml-auto text-[12.5px] text-mist-600">
		{#if board}{fmtNum(board.total)} 名玩家{#if loading}
				· 加载中…{/if}{:else}加载中…{/if}
	</span>
</div>

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<th class="num">#</th>
				<th>玩家</th>
				<SortHeader {sort} key="playtime" num>游戏时间</SortHeader>
				<SortHeader {sort} key="seeded" num title="暖服规则统计的低人数在线时间"
					>种子服时间</SortHeader
				>
				<SortHeader {sort} key="kills" num>K</SortHeader>
				<SortHeader {sort} key="deaths" num>D</SortHeader>
				<SortHeader {sort} key="kd" num>K/D</SortHeader>
				<SortHeader {sort} key="perHour" num title="每小时游戏时间击杀数">K/h</SortHeader>
				<th class="num" title="爆头">HS</th>
				<th class="num" title="误杀队友">TK</th>
				<SortHeader {sort} key="matches" num>比赛记录</SortHeader>
				<SortHeader {sort} key="wins" num title="胜、负、平">W-L-D</SortHeader>
				<SortHeader {sort} key="winRate" num>胜率</SortHeader>
				<SortHeader {sort} key="cash" num>现金</SortHeader>
				<th>最近出现</th>
			</tr>
		</thead>
		<tbody>
			{#each board?.rows ?? [] as r (r.steamId)}
				<tr>
					<td class="num text-mist-400">{r.rank}</td>
					<td>
						<a href={hrefFor(r.steamId)} class="hover:text-accent hover:underline">{r.name}</a>
						{#if showIds}<span class="font-mono text-[12px] text-mist-600">{r.steamId}</span>{/if}
					</td>
					<td class="num">{fmtMinutes(r.minutes)}</td>
					<td class="num">{r.seedMinutes ? fmtMinutes(r.seedMinutes) : '—'}</td>
					<td class="num">{fmtNum(r.kills)}</td>
					<td class="num">{fmtNum(r.deaths)}</td>
					<td class="num">{ratio(kdRatio(r.kills, r.deaths))}</td>
					<td class="num">{ratio(perHour(r.kills, r.minutes), 1)}</td>
					<td class="num">{r.headshots}</td>
					<td class="num {r.teamKills >= 3 ? 'text-warn' : ''}">{r.teamKills}</td>
					<td class="num">{r.matches}</td>
					<td class="num whitespace-nowrap">{r.wins}-{r.losses}-{r.draws}</td>
					<td class="num">{pct(winRate(r.wins, r.losses, r.draws))}</td>
					<td class="num">{fmtCash(r.cash)}</td>
					<td class="whitespace-nowrap text-mist-400">{seen(r.lastSeen)}</td>
				</tr>
			{:else}
				<tr>
					<td colspan="15" class="py-6 text-center text-mist-600">
						{#if !board || loading}加载中…{:else if board.total === 0 && query.minMinutes > 0}还没有人达到
							{fmtMinutes(query.minMinutes)} 的游玩时间。{:else}此时间范围内还没有玩家。{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
{#if board && pages > 1}
	<div class="mt-3 flex items-center gap-2 text-[12.5px] text-mist-400">
		<button
			class="btn btn-sm"
			disabled={query.page <= 1 || loading}
			onclick={() => onchange({ ...query, page: query.page - 1 })}>← 较新</button
		>
		<span>第 {query.page} of {pages}</span>
		<button
			class="btn btn-sm"
			disabled={query.page >= pages || loading}
			onclick={() => onchange({ ...query, page: query.page + 1 })}>下一页 →</button
		>
	</div>
{/if}
{#if board && !board.hasFeed}
	<p class="note">
		{query.scope === 'org' ? '这些服务器均没有' : '此服务器没有'} 击杀事件，因此爆头、队友击杀、自杀和连续击杀未被记录；击杀、死亡及对局结果仍按场次来自游戏计分板。
	</p>
{/if}
