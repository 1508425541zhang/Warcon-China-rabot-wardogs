<script lang="ts">
	import { api, errorMessage } from '$lib/api';
	import { poll } from '$lib/poll';
	import { fmtNum, fmtTime, mapLabel, expSetLabel } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import PopulationChart from '$lib/components/PopulationChart.svelte';
	import CashChart from '$lib/components/CashChart.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort } from '$lib/table.svelte';
	import { factionColor } from '$lib/format';
	import { causeLabel } from '$lib/causes';
	import type { Analytics, Range } from '$lib/server/analytics';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let range = $state<Range>('24h');
	let a = $state<Analytics | null>(null);
	let loading = $state(false);
	let view = $state<'chart' | 'table'>('chart');
	let cashView = $state<'chart' | 'table'>('chart');

	const playerSort = new TableSort<Analytics['players'][number]>({
		player: { by: (p) => p.name },
		minutes: { by: (p) => p.minutes, dir: 'desc' },
		sessions: { by: (p) => p.sessions, dir: 'desc' },
		kills: { by: (p) => p.kills, dir: 'desc' },
		deaths: { by: (p) => p.deaths, dir: 'desc' },
		lastSeen: { by: (p) => p.lastSeen, dir: 'desc' }
	});
	let players = $derived(playerSort.sorted(a?.players ?? []));
	const matchSort = new TableSort<Analytics['matches'][number]>({
		started: { by: (m) => m.startedAt, dir: 'desc' },
		map: { by: (m) => (m.map ? mapLabel(data.catalog, m.map) : null) },
		mode: {
			by: (m) => expSetLabel(data.catalog, m.experiences ? m.experiences.split('+') : [])
		},
		length: {
			by: (m) => Date.parse(m.endedAt ?? new Date().toISOString()) - Date.parse(m.startedAt),
			dir: 'desc'
		},
		peak: { by: (m) => m.peakPlayers, dir: 'desc' },
		result: { by: (m) => m.winner }
	});
	let matchRows = $derived(matchSort.sorted(a?.matches ?? []));
	const combatSort = new TableSort<NonNullable<Analytics['combat']>['players'][number]>(
		{
			player: { by: (p) => p.name },
			kills: { by: (p) => p.kills, dir: 'desc' },
			deaths: { by: (p) => p.deaths, dir: 'desc' },
			headshots: { by: (p) => p.headshots, dir: 'desc' },
			teamKills: { by: (p) => p.teamKills, dir: 'desc' },
			distance: { by: (p) => p.avgDistanceM, dir: 'desc' }
		},
		{ key: 'kills' }
	);
	let combatPlayers = $derived(combatSort.sorted(a?.combat?.players ?? []));
	let maxCauseKills = $derived(Math.max(1, ...(a?.combat?.causes.map((c) => c.kills) ?? [])));
	let maxBucketKills = $derived(Math.max(1, ...(a?.combat?.perBucket.map((b) => b.kills) ?? [])));
	const pct = (part: number, whole: number) =>
		whole ? `${Math.round((part / whole) * 100)}%` : '—';
	const kd = (k: number, d: number) => (d ? (k / d).toFixed(2) : k ? `${k}.00` : '—');

	async function load() {
		loading = true;
		try {
			a = await api<Analytics>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/analytics?range=${range}`
			);
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			loading = false;
		}
	}
	$effect(() => {
		range;
		return poll(load, 60000);
	});

	const RANGES: { key: Range; label: string }[] = [
		{ key: '24h', label: '24 hours' },
		{ key: '7d', label: '7 days' },
		{ key: '30d', label: '30 days' }
	];
	const minutes = (m: number) => (m >= 90 ? `${(m / 60).toFixed(1)} h` : `${m} min`);
	const duration = (from: string, to: string | null) =>
		minutes(Math.round((Date.parse(to ?? new Date().toISOString()) - Date.parse(from)) / 60000));
	let maxMapMinutes = $derived(Math.max(1, ...(a?.maps.map((m) => m.minutes) ?? [])));
	let maxHourly = $derived(Math.max(1, ...(a?.hourly.map((h) => h.avg) ?? [])));
	// Derived rather than inlined in the each: state read only inside a callback of the each
	// expression compiles to non-reactive items, so the bars would freeze on their first values.
	let hourlyBars = $derived(
		Array.from({ length: 24 }, (_, h) => a?.hourly.find((x) => x.hour === h)?.avg ?? 0)
	);
</script>

<div class="mb-4 flex flex-wrap items-center gap-2">
	<div class="join">
		{#each RANGES as r (r.key)}
			<button
				class="btn btn-sm {range === r.key ? 'btn-primary' : ''}"
				onclick={() => (range = r.key)}>{r.label}</button
			>
		{/each}
	</div>
	<span class="ml-auto text-[12.5px] text-mist-600">
		{#if a}{fmtNum(a.summary.samples)} 个采样，覆盖 {a.summary.coveredHours} 小时 · 每 {a.sampleSeconds}秒一次心跳，状态变化时也会更新{#if loading}
				· 刷新中…{/if}{:else}加载中…{/if}
	</span>
</div>

{#if a}
	<div class="mb-4 grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
		{#each [['当前在线', String(a.summary.onlineNow)], ['独立玩家', fmtNum(a.summary.uniquePlayers)], ['Peak', fmtNum(a.summary.peakPlayers)], ['Average', String(a.summary.avgPlayers)], ['Uptime', a.summary.uptimePct === null ? '—' : `${a.summary.uptimePct}%`], ['Matches', fmtNum(a.summary.matches)]] as [label, value] (label)}
			<div class="panel py-4">
				<div class="caps text-mist-400">{label}</div>
				<div class="mt-1 font-display text-3xl font-semibold tabular">{value}</div>
			</div>
		{/each}
	</div>

	<div class="mb-4 panel">
		<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
			<span class="label-sm mb-0">在线玩家</span>
			<span class="text-[12px] text-mist-600"
				>平均每 {a.bucketSeconds / 60} 分钟为一组 · 红色区域表示中断</span
			>
			<span class="join ml-auto">
				<button
					class="btn btn-sm {view === 'chart' ? 'btn-primary' : ''}"
					onclick={() => (view = 'chart')}>图表</button
				>
				<button
					class="btn btn-sm {view === 'table' ? 'btn-primary' : ''}"
					onclick={() => (view = 'table')}>表格</button
				>
			</span>
		</div>
		{#if view === 'chart'}
			<PopulationChart points={a.population} {range} />
		{:else}
			<div class="max-h-[360px] table-wrap">
				<table>
					<thead
						><tr
							><th>时间段起点</th><th class="num">平均值</th><th class="num">峰值</th><th
								class="num">容量</th
							><th class="num">可连接</th></tr
						></thead
					>
					<tbody>
						{#each a.population as p (p.ts)}
							<tr
								><td class="font-mono text-[12px]">{fmtTime(p.ts)}</td><td class="num"
									>{p.avg === null ? '—' : Math.round(p.avg)}</td
								><td class="num">{p.max ?? '—'}</td><td class="num">{p.cap ?? '—'}</td><td
									class="num">{p.ok}/{p.total}</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>

	<div class="mb-4 panel">
		<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
			<span class="label-sm mb-0">场内现金</span>
			<span class="text-[12px] text-mist-600"
				>在线玩家持有的现金 · 每 {a.bucketSeconds / 60} 分钟区间</span
			>
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
			points={a.cash}
			view={cashView}
			{range}
			color={(name) => factionColor(name, null)}
			emptyText="No cash samples in this range yet. The poller records cash per faction with every sample."
		/>
	</div>

	<div class="mb-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="panel">
			<span class="label-sm">每张地图时长</span>
			{#each a.maps as m (m.map)}
				<div class="mb-2.5">
					<div class="mb-1 flex justify-between text-[13px]">
						<span
							>{mapLabel(data.catalog, m.map)}
							<span class="text-mist-600">· {m.matches} 场对局</span></span
						><span class="font-mono text-mist-400 tabular">{minutes(m.minutes)}</span>
					</div>
					<div class="progress">
						<span class="progress-bar" style="width:{(m.minutes / maxMapMinutes) * 100}%"></span>
					</div>
				</div>
			{:else}
				<div class="text-mist-600">暂无数据。</div>
			{/each}
		</div>
		<div class="panel">
			<span class="label-sm">每小时平均人数（UTC）</span>
			{#if a.hourly.length}
				<div class="flex h-36 items-end gap-[3px]" role="img" aria-label="按小时统计的平均人数">
					{#each hourlyBars as v, h (h)}
						<div class="group relative flex-1">
							<div
								class="w-full bg-accent/80 transition-[height]"
								style="height:{Math.max(2, (v / maxHourly) * 130)}px"
								title="{String(h).padStart(2, '0')}:00 · {v.toFixed(1)} avg"
							></div>
						</div>
					{/each}
				</div>
				<div class="mt-1 flex justify-between font-mono text-[10px] text-mist-600">
					<span>00</span><span>06</span><span>12</span><span>18</span><span>23</span>
				</div>
			{:else}
				<div class="text-mist-600">暂无数据。</div>
			{/if}
		</div>
	</div>

	<div class="mb-4 panel">
		<span class="label-sm">最活跃玩家</span>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<SortHeader sort={playerSort} key="player">玩家</SortHeader>
						<SortHeader sort={playerSort} key="minutes" num>游戏时间</SortHeader>
						<SortHeader sort={playerSort} key="sessions" num>场次</SortHeader>
						<SortHeader sort={playerSort} key="kills" num>K</SortHeader>
						<SortHeader sort={playerSort} key="deaths" num>D</SortHeader>
						<SortHeader sort={playerSort} key="lastSeen">最近出现</SortHeader>
					</tr>
				</thead>
				<tbody>
					{#each players as p (p.steamId)}
						<tr>
							<td
								>{p.name} <span class="font-mono text-[12px] text-mist-600">{p.steamId}</span>
								{#if p.online}<Badge tone="ok" class="ml-1">在线</Badge>{/if}</td
							>
							<td class="num">{minutes(p.minutes)}</td><td class="num">{p.sessions}</td><td
								class="num">{fmtNum(p.kills)}</td
							><td class="num">{fmtNum(p.deaths)}</td>
							<td class="whitespace-nowrap text-mist-400">{fmtTime(p.lastSeen)}</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-6 text-center text-mist-600">所选时间内暂无玩家场次。</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
	</div>

	{#if a.combat}
		<div class="mb-4 panel">
			<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
				<span class="label-sm mb-0">战斗数据</span>
				<span class="text-[12px] text-mist-600"
					>来自游戏击杀事件 · 每 {a.bucketSeconds / 60} 分钟区间</span
				>
			</div>
			<div class="mb-4 grid grid-cols-2 gap-3 md:grid-cols-5">
				{#each [['Kills', fmtNum(a.combat.kills)], ['Headshots', `${fmtNum(a.combat.headshots)} · ${pct(a.combat.headshots, a.combat.kills)}`], ['队友击杀', fmtNum(a.combat.teamKills)], ['Suicides', fmtNum(a.combat.suicides)], ['By vehicle', fmtNum(a.combat.vehicleKills)]] as [label, value] (label)}
					<div class="rounded-ctl border border-black bg-ink-950 px-3.5 py-3">
						<div class="caps text-mist-400">{label}</div>
						<div class="mt-1 font-display text-2xl font-semibold tabular">{value}</div>
					</div>
				{/each}
			</div>
			{#if a.combat.perBucket.length}
				<div class="flex h-28 items-end gap-[2px]" role="img" aria-label="每时间段击杀数">
					{#each a.combat.perBucket as b (b.ts)}
						<div
							class="min-w-[2px] flex-1 bg-accent/80"
							style="height:{Math.max(2, (b.kills / maxBucketKills) * 100)}px"
							title="{fmtTime(b.ts)} · {b.kills} 次击杀"
						></div>
					{/each}
				</div>
			{:else}
				<div class="text-mist-600">所选时间内暂无击杀记录。</div>
			{/if}
		</div>

		<div class="mb-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
			<div class="panel">
				<span class="label-sm">武器与载具</span>
				{#each a.combat.causes as c (c.cause)}
					<div class="mb-2.5">
						<div class="mb-1 flex justify-between text-[13px]">
							<span
								>{causeLabel(c.cause)}
								<span class="text-mist-600">· {pct(c.headshots, c.kills)} 次爆头</span></span
							><span class="font-mono text-mist-400 tabular">{fmtNum(c.kills)}</span>
						</div>
						<div class="progress">
							<span class="progress-bar" style="width:{(c.kills / maxCauseKills) * 100}%"></span>
						</div>
					</div>
				{:else}
					<div class="text-mist-600">暂无数据。</div>
				{/each}
			</div>
			<div class="panel">
				<span class="label-sm">最长连续击杀</span>
				<div class="table-wrap">
					<table>
						<thead
							><tr
								><th>时间</th><th>击杀者</th><th>受害者</th><th>击杀原因</th><th class="num"
									>距离</th
								></tr
							></thead
						>
						<tbody>
							{#each a.combat.longest as l (l.ts + l.killer + l.victim)}
								<tr>
									<td class="whitespace-nowrap text-mist-400">{fmtTime(l.ts)}</td>
									<td>{l.killer}</td><td>{l.victim}</td>
									<td>{causeLabel(l.cause) || '—'}</td>
									<td class="num">{fmtNum(l.distanceM)} m</td>
								</tr>
							{:else}
								<tr><td colspan="5" class="py-6 text-center text-mist-600">暂无数据。</td></tr>
							{/each}
						</tbody>
					</table>
				</div>
			</div>
		</div>

		<div class="mb-4 panel">
			<span class="label-sm">击杀榜</span>
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							<SortHeader sort={combatSort} key="player">玩家</SortHeader>
							<SortHeader sort={combatSort} key="kills" num>K</SortHeader>
							<SortHeader sort={combatSort} key="deaths" num>D</SortHeader>
							<th class="num">K/D</th>
							<SortHeader sort={combatSort} key="headshots" num>爆头</SortHeader>
							<SortHeader sort={combatSort} key="teamKills" num>队友击杀</SortHeader>
							<SortHeader sort={combatSort} key="distance" num>平均距离</SortHeader>
						</tr>
					</thead>
					<tbody>
						{#each combatPlayers as p (p.steamId)}
							<tr class={p.teamKills >= 3 ? 'text-warn' : ''}>
								<td
									><a
										href="/server/{encodeURIComponent(id)}/players/{p.steamId}"
										class="hover:text-accent hover:underline">{p.name}</a
									>
									<span class="font-mono text-[12px] text-mist-600">{p.steamId}</span></td
								>
								<td class="num">{fmtNum(p.kills)}</td><td class="num">{fmtNum(p.deaths)}</td>
								<td class="num">{kd(p.kills, p.deaths)}</td>
								<td class="num">{p.headshots} · {pct(p.headshots, p.kills)}</td>
								<td class="num">{p.teamKills}</td>
								<td class="num">{p.avgDistanceM === null ? '—' : `${p.avgDistanceM} m`}</td>
							</tr>
						{:else}
							<tr
								><td colspan="7" class="py-6 text-center text-mist-600">所选时间内暂无击杀记录。</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	{/if}

	<div class="panel">
		<span class="label-sm">比赛记录</span>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<SortHeader sort={matchSort} key="started">开始时间</SortHeader>
						<SortHeader sort={matchSort} key="map">地图</SortHeader>
						<SortHeader sort={matchSort} key="mode">模式与模组</SortHeader>
						<SortHeader sort={matchSort} key="length" num>时长</SortHeader>
						<SortHeader sort={matchSort} key="peak" num>峰值</SortHeader>
						<SortHeader sort={matchSort} key="result">结果</SortHeader>
					</tr>
				</thead>
				<tbody>
					{#each matchRows as m (m.id)}
						<tr>
							<td class="whitespace-nowrap">{fmtTime(m.startedAt)}</td>
							<td>{m.map ? mapLabel(data.catalog, m.map) : '—'}</td>
							<td>{expSetLabel(data.catalog, m.experiences ? m.experiences.split('+') : [])}</td>
							<td class="num"
								>{duration(m.startedAt, m.endedAt)}{#if !m.endedAt}<Badge tone="info" class="ml-1"
										>实时</Badge
									>{/if}</td
							>
							<td class="num">{m.peakPlayers}</td>
							<td>
								{#if m.finalScores}
									{#if m.winner}<b>{m.winner}</b> ·
									{/if}<span class="text-mist-400"
										>{m.finalScores.map((s) => `${s.name} ${s.score}`).join(' · ')}</span
									>
								{:else}<span class="text-mist-600">—</span>{/if}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-6 text-center text-mist-600">所选时间内暂无比赛记录。</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="note">
			比赛边界根据两次采样间的比赛时钟与地图变化推断，因此时长误差最多为一个轮询周期。
		</p>
	</div>
{/if}
