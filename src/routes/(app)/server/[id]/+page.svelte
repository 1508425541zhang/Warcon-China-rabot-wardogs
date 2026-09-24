<script lang="ts">
	import { scoreCapOf } from '$lib/match';
	import { rconGet, rconPost, errorMessage } from '$lib/api';
	import { poll } from '$lib/poll';
	import { watchLive, type KillsNotice } from '$lib/live';
	import { causeLabel } from '$lib/causes';
	import { expSetLabel, fmtNum, lightingLabel, mapLabel, zoneLabel } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { setHealth } from '$lib/health.svelte';
	import MapPicker from '$lib/components/MapPicker.svelte';
	import MapArt from '$lib/components/MapArt.svelte';
	import { sponsor, loadSponsor } from '$lib/sponsor.svelte';
	import FactionChip from '$lib/components/FactionChip.svelte';
	import CashChart from '$lib/components/CashChart.svelte';
	import { cashByFaction } from '$lib/cash';
	import { api } from '$lib/api';
	import { factionColor } from '$lib/format';
	import type { CashPoint } from '$lib/server/analytics';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort } from '$lib/table.svelte';
	import type { KillView, LiveView, Player, Rotation, Status } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let chat = $derived(can(data.server.caps, 'chat.send'));
	// The banner the server advertises to the game's browser; the config page edits it.
	let banner = $derived(sponsor[data.server.id] ?? '');
	$effect(() => {
		void loadSponsor(data.server.id);
	});
	let match = $derived(can(data.server.caps, 'match.control'));

	let status = $state<Status | null>(null);
	let rotation = $state<Rotation | null>(null);
	let players = $state<Player[]>([]);
	let live = $state<LiveView | null>(null);
	let playersSeenAt = '';
	let teamFilter = $state('');
	let now = $state(Date.now());
	let showPicker = $state(false);
	let broadcast = $state('');
	let picker = $state<MapPicker>();
	let seeded = false;

	// Cash in play for the current match: one point per players poll, seeded from the poller's
	// samples since the match started so a page load does not begin with an empty chart. Cleared
	// when the map changes or the match clock jumps back (a restart).
	const CASH_POINTS_MAX = 1500;
	let cash = $state<CashPoint[]>([]);
	let cashView = $state<'chart' | 'table'>('chart');
	let cashSeeded = false;
	let prevMap: string | null = null;
	let prevMatchSeconds = Infinity;

	async function seedCash(matchSeconds: number) {
		cashSeeded = true;
		try {
			const since = new Date(Date.now() - matchSeconds * 1000).toISOString();
			const r = await api<{ points: CashPoint[] }>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/cash?since=${encodeURIComponent(since)}`
			);
			const firstLive = cash.length ? Date.parse(cash[0].ts) : Infinity;
			cash = [...r.points.filter((p) => Date.parse(p.ts) < firstLive), ...cash];
		} catch {
			/* the chart fills in from live polls */
		}
	}
	function noteCashStatus(s: Status) {
		const restarted =
			prevMap !== null && (s.map !== prevMap || (s.matchSeconds ?? 0) < prevMatchSeconds - 5);
		prevMap = s.map;
		prevMatchSeconds = s.matchSeconds ?? 0;
		if (restarted) {
			cash = [];
			cashSeeded = false;
		}
		if (!cashSeeded && s.matchSeconds !== null) void seedCash(s.matchSeconds);
	}
	function noteCashSample(list: Player[]) {
		const factions: Record<string, number> = {};
		let total = 0;
		for (const f of cashByFaction(status, list)) {
			factions[f.name] = f.cash;
			total += f.cash;
		}
		const next = [...cash, { ts: new Date().toISOString(), total, factions }];
		cash = next.length > CASH_POINTS_MAX ? next.slice(next.length - CASH_POINTS_MAX) : next;
	}

	async function act(
		action: string,
		params: object,
		opts: { confirm?: string; danger?: boolean; after?: () => Promise<unknown> } = {}
	) {
		if (
			opts.confirm &&
			!(await confirmDialog(opts.confirm, { okLabel: '确认执行', danger: opts.danger }))
		)
			return null;
		try {
			const result = await rconPost<{ message?: string }>(id, action, params);
			toast(result?.message || '操作已完成。', 'ok');
			if (opts.after) await opts.after();
			return result;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return null;
		}
	}

	// Status and players come from the worker's observations (an event stream, a few times a
	// second while this page is open); the rotation is read on load, after a command, and now and then.
	function onLive(v: LiveView) {
		live = v;
		setHealth(id, v.ok);
		if (v.status) {
			status = v.status;
			noteCashStatus(v.status);
			if (!seeded && picker) {
				seeded = true;
				void picker.setFrom({
					map: v.status.map,
					experiences: v.status.experiences,
					lighting: v.status.lighting,
					zoneAlternator: v.status.alternator
				});
			}
		}
		if (v.playersAt && v.playersAt !== playersSeenAt) {
			playersSeenAt = v.playersAt;
			players = v.players;
			noteCashSample(players);
		}
	}
	// The kill feed: the stored tail on load, then every batch as the game delivers it (the event
	// stream), with a slow re-read behind it in case a frame was missed.
	const KILLS_MAX = 100;
	let kills = $state<KillView[]>([]);
	let feedConfigured = $state<boolean | null>(null);
	let feedAt = $state<string | null>(null);
	async function loadKills() {
		try {
			const r = await api<{ configured: boolean; feedAt: string | null; kills: KillView[] }>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/kills?limit=${KILLS_MAX}`
			);
			feedConfigured = r.configured;
			feedAt = r.feedAt;
			kills = r.kills;
		} catch {
			/* the panel keeps what it has */
		}
	}
	function onKills(n: KillsNotice) {
		if (n.serverId !== id || !n.kills.length) return;
		const known = new Set(kills.map((k) => k.eventId));
		const fresh = n.kills.filter((k) => !known.has(k.eventId)).reverse();
		kills = [...fresh, ...kills].slice(0, KILLS_MAX);
		feedAt = n.kills[n.kills.length - 1].ts;
	}
	let feedAgeS = $derived(
		feedAt ? Math.max(0, Math.round((now - Date.parse(feedAt)) / 1000)) : null
	);
	const clock = (iso: string) => new Date(iso).toLocaleTimeString('zh-CN', { hour12: false });
	async function refreshRotation() {
		try {
			rotation = await rconGet<Rotation>(id, 'rotation');
		} catch (err) {
			toast(errorMessage(err), 'err');
		}
	}
	/** After a command: the worker looks again on its own; the rotation is ours to re-read. */
	const refreshStatus = refreshRotation;

	$effect(() => {
		const stops = [
			watchLive([id], onLive, undefined, onKills),
			poll(refreshRotation, 30000),
			poll(loadKills, 60000)
		];
		const t = setInterval(() => (now = Date.now()), 1000);
		return () => {
			stops.forEach((s) => s());
			clearInterval(t);
		};
	});

	let scoreScale = $derived(scoreCapOf(status));
	let next = $derived(
		rotation && rotation.enabled && rotation.nextIndex >= 0
			? rotation.entries[rotation.nextIndex]
			: null
	);
	let teams = $derived.by(() => {
		const m = new Map<string, number>();
		for (const p of players) m.set(p.faction || '', (m.get(p.faction || '') || 0) + 1);
		return [...m.entries()];
	});
	/** the scoreboard's own order, kills then fewest deaths, is what a header sort layers on */
	const boardSort = new TableSort<Player>(
		{
			player: { by: (p) => p.name },
			faction: { by: (p) => p.faction },
			kills: { by: (p) => p.kills, dir: 'desc' },
			deaths: { by: (p) => p.deaths, dir: 'desc' },
			cash: { by: (p) => p.cash, dir: 'desc' },
			ping: { by: (p) => p.ping }
		},
		{ key: 'kills' }
	);
	let board = $derived(
		boardSort.sorted(
			players
				.filter((p) => !teamFilter || (p.faction || 'unassigned') === teamFilter)
				.sort((a, b) => b.kills - a.kills || a.deaths - b.deaths)
		)
	);

	async function sendBroadcast() {
		const message = broadcast.trim();
		if (!message) return;
		if (await act('broadcast', { message })) broadcast = '';
	}
</script>

<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
	<div class="panel">
		<span class="label-sm">分数</span>
		{#if status}
			<MapArt
				map={status.map}
				lighting={status.lighting}
				variant="wide"
				alt="{mapLabel(data.catalog, status.map)}, {lightingLabel(data.catalog, status.lighting)}"
				class="mb-3"
			/>
			<div class="mb-3 space-y-2.5">
				{#each status.scores as f (f.name)}
					{@const pct = Math.min(100, Math.max(0, Math.round((f.score / scoreScale) * 100)))}
					<div>
						<div class="mb-1 flex items-center justify-between text-[13px]">
							<span class="inline-flex items-center gap-1.5 font-medium"
								><span class="inline-block h-2.5 w-2.5 rounded-full" style="background:{f.colorHex}"
								></span>{f.name}</span
							>
							<span class="font-mono tabular">{fmtNum(f.score)}</span>
						</div>
						<div class="progress">
							<span class="progress-bar" style="width:{pct}%;background:{f.colorHex || ''}"></span>
						</div>
					</div>
				{/each}
			</div>
			<div class="kv">
				<span class="text-mist-400">服务器</span><span class="truncate"
					>{status.serverName || '—'}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">玩家</span><span
					>{fmtNum(status.playerCount)} / {fmtNum(status.maxPlayers)}{#if live?.reservedSlots}<span
							class="text-mist-400">&nbsp;+ {live.reservedSlots} 个预留位</span
						>{/if}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">地图</span><span>{mapLabel(data.catalog, status.map)}</span>
			</div>
			<div class="kv">
				<span class="text-mist-400">游戏模式与模组</span><span class="text-right"
					>{expSetLabel(data.catalog, status.experiences)}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">时间与天气</span><span
					>{lightingLabel(data.catalog, status.lighting)}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">控制区</span><span>{zoneLabel(status.alternator)}</span>
			</div>
			<div class="kv">
				<span class="text-mist-400">得分周期</span><span
					>{status.scoreTick !== null
						? `${status.scoreTick} 秒（范围 ${status.scoreTickMin}–${status.scoreTickMax}）`
						: '—'}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">分数上限</span><span
					>{scoreScale}{#if status.scoreCap === null}
						<span class="text-mist-600">（游戏默认值）</span>{/if}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">地图轮换</span>
				<span
					>{rotation
						? rotation.enabled
							? `${rotation.mode}，当前第 ${rotation.nowIndex + 1} 项${rotation.nextIndex >= 0 ? `，下一项第 ${rotation.nextIndex + 1} 项` : ''}，共 ${rotation.entries.length} 项`
							: '关闭'
						: '—'}</span
				>
			</div>
		{:else}
			<div class="text-mist-600">加载中…</div>
		{/if}
	</div>

	<div class="panel">
		<span class="label-sm">比赛控制</span>
		<div class="stat-big">
			<span class="text-mist-400">下一张地图</span>
			<span class="inline-flex items-center gap-3 text-right text-lg font-semibold"
				>{next
					? mapLabel(data.catalog, next.map)
					: rotation && !rotation.enabled && status
						? `${mapLabel(data.catalog, status.map)}（轮换已关闭，仍为当前地图）`
						: '—'}{#if next}<MapArt
						map={next.map}
						lighting={next.lighting}
						variant="720"
						alt=""
						class="w-16 shrink-0"
					/>{/if}</span
			>
		</div>
		{#if next && rotation}
			<p class="note mt-0">
				轮换第 {rotation.nextIndex + 1} / {rotation.entries.length} 项 · {expSetLabel(
					data.catalog,
					next.experiences
				)} · {lightingLabel(data.catalog, next.lighting)}
			</p>
		{/if}
		<div class="join join-stack mt-3">
			<button class="btn" disabled={!match} onclick={() => (showPicker = !showPicker)}
				>指定地图</button
			>
			<button
				class="btn"
				disabled={!match}
				onclick={() =>
					act(
						'restartMatch',
						{},
						{
							confirm: '重开当前比赛？分数会重置，地图轮换位置不变。',
							after: refreshStatus
						}
					)}>重开比赛</button
			>
			<button
				class="btn btn-danger"
				disabled={!match}
				onclick={() =>
					act(
						'endMatch',
						{},
						{
							confirm: '强制结束比赛？下一张地图由轮换决定；若轮换关闭，将重新加载当前地图。',
							danger: true,
							after: refreshStatus
						}
					)}>强制结束比赛</button
			>
		</div>
		<p class="note">
			{chat || match ? '地图切换和比赛结束命令会在结算画面结束后执行。' : '你只有本服查看权限。'}
		</p>
		<form
			class="mt-4"
			onsubmit={(e) => {
				e.preventDefault();
				void sendBroadcast();
			}}
		>
			<span class="field-label">向所有玩家广播</span>
			<div class="join w-full">
				<input
					class="input"
					type="text"
					maxlength="200"
					placeholder="向全服玩家显示的消息…"
					bind:value={broadcast}
					disabled={!chat}
				/>
				<button class="btn btn-primary" type="submit" disabled={!chat}>发送</button>
			</div>
		</form>
		{#if banner}
			<div class="mt-4">
				<span class="field-label">服务器图片</span>
				{#key banner}
					<img
						src={banner}
						alt="服务器横幅"
						class="h-16 w-auto max-w-full rounded border border-black object-cover"
						loading="lazy"
						referrerpolicy="no-referrer"
						onerror={(e) => ((e.currentTarget as HTMLImageElement).hidden = true)}
					/>
				{/key}
			</div>
		{/if}
	</div>
</div>

<div class="mt-4 panel">
	<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
		<span class="label-sm mb-0">场内现金</span>
		<span class="text-[12px] text-mist-600">本场在线玩家持有的现金 · 每次观测记一个点</span>
		<span class="join ml-auto">
			<button
				class="btn btn-sm {cashView === 'chart' ? 'btn-primary' : ''}"
				onclick={() => (cashView = 'chart')}>图表</button
			>
			<button
				class="btn btn-sm {cashView === 'table' ? 'btn-primary' : ''}"
				onclick={() => (cashView = 'table')}>表格</button
			>
		</span>
	</div>
	<CashChart
		points={cash}
		view={cashView}
		color={(name) => factionColor(name, status?.scores)}
		emptyText="暂无现金样本；计分板刷新后会显示数据。"
	/>
</div>

<div class="mt-4 panel" hidden={!showPicker}>
	<span class="label-sm">指定地图</span>
	<MapPicker bind:this={picker} serverId={id} catalog={data.catalog} disabled={!match} />
	<div class="join join-stack mt-4">
		<button
			class="btn btn-primary"
			disabled={!match || !data.features.rotationEdit}
			title={data.features.rotationEdit
				? ''
				: '当前服务器版本没有地图轮换编辑接口，无法预设下一张地图。'}
			onclick={() => picker && act('setNextMap', picker.selection(), { after: refreshStatus })}
			>设为下一张地图</button
		>
		<button
			class="btn btn-danger"
			disabled={!match}
			onclick={() =>
				picker &&
				act('changeMap', picker.selection(), {
					confirm: '结束当前回合，并在结算画面结束后切换到所选地图？',
					danger: true,
					after: refreshStatus
				})}>立即切换地图</button
		>
	</div>
	<p class="note">
		设为下一张地图会修改轮换顺序：所选地图将成为下一场地图，必要时会加入轮换列表。在“地图轮换”页保存后，重启服务器也会保留该设置。
	</p>
</div>

<div class="mt-4 panel">
	<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
		<span class="label-sm mb-0">计分板</span>
		<div class="join">
			<button
				class="btn btn-sm {teamFilter === '' ? 'btn-primary' : ''}"
				onclick={() => (teamFilter = '')}>全部 {players.length}</button
			>
			{#each teams as [f, n] (f)}
				<button
					class="btn btn-sm {teamFilter === (f || 'unassigned') ? 'btn-primary' : ''}"
					onclick={() => (teamFilter = f || 'unassigned')}>{f || '未分配'} {n}</button
				>
			{/each}
		</div>
		<span class="ml-auto text-[12.5px] text-mist-600"
			>本服在线 {players.length} 人{#if live?.playersAt}
				· {Math.max(0, Math.round((now - Date.parse(live.playersAt)) / 1000))} 秒前更新{/if}</span
		>
	</div>
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<SortHeader sort={boardSort} key="player">玩家</SortHeader>
					<SortHeader sort={boardSort} key="faction">阵营</SortHeader>
					<SortHeader sort={boardSort} key="kills" num>K</SortHeader>
					<SortHeader sort={boardSort} key="deaths" num>D</SortHeader>
					<SortHeader sort={boardSort} key="cash" num>现金</SortHeader>
					<SortHeader sort={boardSort} key="ping" num>延迟</SortHeader>
				</tr>
			</thead>
			<tbody>
				{#each board as p (p.steamId)}
					<tr>
						<td>{p.name} <span class="font-mono text-[12px] text-mist-600">{p.steamId}</span></td>
						<td><FactionChip faction={p.faction} scores={status?.scores} /></td>
						<td class="num">{p.kills}</td><td class="num">{p.deaths}</td>
						<td class="num">{fmtNum(p.cash)}</td><td class="num">{p.ping ?? '—'}</td>
					</tr>
				{:else}
					<tr><td colspan="6" class="py-6 text-center text-mist-600">当前没有玩家在线。</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>

{#if feedConfigured || kills.length}
	<div class="mt-4 panel">
		<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
			<span class="label-sm mb-0">击杀事件</span>
			<span class="text-[12px] text-mist-600">来自游戏击杀事件 · 最新优先</span>
			<a
				href="/server/{encodeURIComponent(id)}/kills"
				class="text-[12.5px] text-accent hover:underline">查看全部击杀并筛选 →</a
			>
			<span
				class="ml-auto text-[12.5px] {feedAgeS !== null && feedAgeS > 900 && players.length
					? 'text-warn'
					: 'text-mist-600'}"
			>
				{#if feedAgeS === null}尚未收到击杀事件{:else if feedAgeS > 900 && players.length}有玩家在线，但已
					{Math.round(feedAgeS / 60)} 分钟无击杀事件{:else}最近事件：{feedAgeS} 秒前{/if}
			</span>
		</div>
		<div class="max-h-[420px] table-wrap">
			<table>
				<thead>
					<tr
						><th>时间</th><th>击杀者</th><th>受害者</th><th>击杀原因</th><th class="num">距离</th
						><th></th></tr
					>
				</thead>
				<tbody>
					{#each kills as k (k.eventId)}
						<tr class={k.teamKill ? 'text-warn' : ''}>
							<td class="font-mono text-[12px] whitespace-nowrap text-mist-400">{clock(k.ts)}</td>
							<td>
								{#if k.killer}
									<a
										href="/server/{encodeURIComponent(id)}/players/{k.killer.steamId}"
										class="hover:text-accent hover:underline">{k.killer.name}</a
									>
									{#if k.killer.faction}<FactionChip
											faction={k.killer.faction}
											scores={status?.scores}
										/>{/if}
								{:else}<span class="text-mist-600">—</span>{/if}
							</td>
							<td>
								<a
									href="/server/{encodeURIComponent(id)}/players/{k.victim.steamId}"
									class="hover:text-accent hover:underline">{k.victim.name}</a
								>
								{#if k.victim.faction}<FactionChip
										faction={k.victim.faction}
										scores={status?.scores}
									/>{/if}
							</td>
							<td class="text-mist-200"
								>{causeLabel(k.cause) || (k.tags.includes('Falling') ? '坠落' : '—')}</td
							>
							<td class="num">{k.distanceM === null ? '—' : `${Math.round(k.distanceM)} m`}</td>
							<td class="whitespace-nowrap">
								{#if k.teamKill}<span class="chip">误杀队友</span>{/if}
								{#if k.suicide}<span class="chip">自杀</span>{/if}
								{#if k.headshot}<span class="chip">爆头</span>{/if}
								{#each k.tags as t (t)}<span class="chip"
										>{t.replace(/([a-z])([A-Z])/g, '$1 $2').toLowerCase()}</span
									>{/each}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-6 text-center text-mist-600"
								>尚未收到击杀事件；游戏上报后会显示在这里。</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
{/if}
