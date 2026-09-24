<script lang="ts">
	// Everyone who has played on this server, from its own sessions: the Players tab's other view.
	// The server does the searching and the ordering; this holds the filters and the page of rows.
	import { api, qs, errorMessage } from '$lib/api';
	import { fmtAgo, fmtDuration, fmtNum, fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import BanDialog from '$lib/components/BanDialog.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import type { SortLike } from '$lib/table.svelte';
	import type { SeenPlayer, SeenSort } from '$lib/server/seen';

	let {
		server,
		canBan,
		canWatch,
		canOrg
	}: {
		server: { id: string; name: string; orgId: string; orgName: string };
		/** holds Bans here: the row's Ban places it on this server's list or the org's */
		canBan: boolean;
		/** holds Notes here: the row's Watch puts the player on the org's watchlist */
		canWatch: boolean;
		/** may edit the org's ban list: the Ban dialog then offers every server */
		canOrg: boolean;
	} = $props();

	const PAGE = 50;
	let q = $state('');
	let since = $state('1');
	let flag = $state('');
	let sortKey = $state<SeenSort>('lastSeen');
	let dir = $state<'asc' | 'desc'>('desc');
	let rows = $state<SeenPlayer[]>([]);
	let total = $state(0);
	let loading = $state(true);
	let busy = $state('');
	let banning = $state<SeenPlayer | null>(null);

	let apiBase = $derived(`/api/servers/${encodeURIComponent(server.id)}/players/seen`);
	const query = () => ({ q: q.trim(), since, flag, sort: sortKey, dir });

	/** the newest request wins: a slow answer to an earlier search never lands over a later one */
	let asked = 0;
	async function load(offset = 0) {
		const mine = ++asked;
		loading = true;
		try {
			const d = await api<{ players: SeenPlayer[]; total: number }>(
				'GET',
				`${apiBase}${qs({ ...query(), offset, limit: PAGE })}`
			);
			if (mine !== asked) return;
			rows = offset ? [...rows, ...d.players] : d.players;
			total = d.total;
		} catch (err) {
			if (mine === asked) toast(errorMessage(err), 'err');
		} finally {
			if (mine === asked) loading = false;
		}
	}
	let timer: ReturnType<typeof setTimeout> | undefined;
	const loadSoon = () => {
		clearTimeout(timer);
		timer = setTimeout(() => void load(), 300);
	};
	$effect(() => {
		void server.id;
		void load();
		return () => clearTimeout(timer);
	});

	const sort: SortLike<SeenSort> = {
		get key() {
			return sortKey;
		},
		get dir() {
			return dir;
		},
		toggle(key) {
			dir = sortKey === key ? (dir === 'asc' ? 'desc' : 'asc') : key === 'name' ? 'asc' : 'desc';
			sortKey = key;
			void load();
		}
	};

	async function watch(p: SeenPlayer) {
		busy = p.steamId;
		try {
			await api('PUT', `/api/servers/${encodeURIComponent(server.id)}/players/${p.steamId}/watch`, {
				watched: !p.watched,
				reason: ''
			});
			toast(
				p.watched ? `${p.name} taken off the watchlist.` : `${p.name} is on the watchlist.`,
				'ok'
			);
			await load();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = '';
		}
	}
	const SINCE_LABEL: Record<string, string> = {
		'': 'ever',
		'1': '最近 1 天',
		'7': '最近 7 天',
		'30': '最近 30 天',
		'90': '最近 90 天'
	};
</script>

<div class="mb-3 flex flex-wrap items-center gap-2">
	<input
		class="input w-full sm:w-80"
		type="search"
		placeholder="名称、曾用名或 SteamID…"
		aria-label="搜索历史玩家"
		bind:value={q}
		oninput={loadSoon}
	/>
	<select
		class="input w-full sm:w-40"
		aria-label="出现时间范围"
		bind:value={since}
		onchange={() => load()}
	>
		<option value="1">最近 24 小时</option>
		<option value="7">最近 7 天</option>
		<option value="30">最近 30 天</option>
		<option value="90">最近 90 天</option>
		<option value="">全部时间</option>
	</select>
	<select class="input w-full sm:w-40" aria-label="标记" bind:value={flag} onchange={() => load()}>
		<option value="">所有人</option>
		<option value="online">当前在线</option>
		<option value="banned">已封禁</option>
		<option value="watched">在观察名单中</option>
	</select>
	<span class="text-[12.5px] text-mist-600 sm:ml-auto"
		>{#if loading && !rows.length}搜索中…{:else}{fmtNum(rows.length)} of {fmtNum(total)} 名玩家曾在此游玩
			{SINCE_LABEL[since]}{/if}</span
	>
</div>
<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<SortHeader {sort} key="name">玩家</SortHeader>
				<SortHeader {sort} key="lastSeen">最近出现</SortHeader>
				<SortHeader {sort} key="firstSeen">首次出现</SortHeader>
				<SortHeader {sort} key="sessions" num>场次</SortHeader>
				<SortHeader {sort} key="minutes" num>游戏时间</SortHeader>
				<SortHeader {sort} key="kills" num>K</SortHeader>
				<SortHeader {sort} key="deaths" num>D</SortHeader>
				<th></th>
				{#if canBan || canWatch}<th></th>{/if}
			</tr>
		</thead>
		<tbody>
			{#each rows as p (p.steamId)}
				<tr>
					<td>
						<div class="flex flex-wrap items-baseline gap-x-2">
							<a
								href="/server/{encodeURIComponent(server.id)}/players/{p.steamId}"
								class="font-medium hover:text-accent hover:underline"
								title="打开资料">{p.name}</a
							>
							{#if p.aliases.length}
								<span class="text-[12px] text-mist-600" title={p.aliases.join(', ')}
									>另有： {p.aliases.slice(0, 3).join(', ')}{#if p.aliases.length > 3}
										+{p.aliases.length - 3}{/if}</span
								>
							{/if}
						</div>
						<div class="font-mono text-[12.5px] text-mist-400">{p.steamId}</div>
					</td>
					<td class="whitespace-nowrap" title={fmtTime(p.lastSeen)}>{fmtAgo(p.lastSeen)}</td>
					<td class="whitespace-nowrap text-mist-400">{fmtTime(p.firstSeen)}</td>
					<td class="num">{fmtNum(p.sessions)}</td>
					<td class="num whitespace-nowrap">{fmtDuration(p.minutes * 60)}</td>
					<td class="num">{fmtNum(p.kills)}</td>
					<td class="num">{fmtNum(p.deaths)}</td>
					<td class="whitespace-nowrap">
						<span class="inline-flex gap-1">
							{#if p.online}<Badge tone="ok">在线</Badge>{/if}
							{#if p.banned}<Badge tone="warn">{p.banned === 'org' ? 'banned' : '本服已封禁'}</Badge
								>{/if}
							{#if p.watched}<Badge tone="info">已观察</Badge>{/if}
						</span>
					</td>
					{#if canBan || canWatch}
						<td class="py-1.5 text-right whitespace-nowrap">
							<span class="inline-flex gap-1.5">
								{#if canWatch}
									<button class="btn btn-sm" disabled={busy === p.steamId} onclick={() => watch(p)}
										>{p.watched ? 'Unwatch' : 'Watch'}</button
									>
								{/if}
								{#if canBan}
									<button
										class="btn btn-sm btn-danger"
										disabled={!!p.banned}
										onclick={() => (banning = p)}>封禁</button
									>
								{/if}
							</span>
						</td>
					{/if}
				</tr>
			{:else}
				<tr
					><td colspan="9" class="py-6 text-center text-mist-600"
						>{loading ? 'Searching…' : '没有符合条件的玩家曾在此游玩。'}</td
					></tr
				>
			{/each}
		</tbody>
	</table>
</div>
{#if rows.length < total}
	<div class="mt-3 text-center">
		<button class="btn btn-sm" disabled={loading} onclick={() => load(rows.length)}>查看更多</button
		>
	</div>
{/if}
<p class="note">
	根据本服务器的会话历史，显示所有曾在此游玩的玩家。未曾加入的 SteamID
	无法在这里找到。点击昵称可打开玩家档案{#if canBan}；封禁会加入此服务器的列表，并在玩家下次进入时生效{/if}.
</p>

{#if banning}
	{#key banning.steamId}
		<BanDialog
			orgId={server.orgId}
			orgName={server.orgName}
			steamId={banning.steamId}
			name={banning.name}
			server={{ id: server.id, name: server.name }}
			{canOrg}
			onclose={() => (banning = null)}
			ondone={() => load()}
		/>
	{/key}
{/if}
