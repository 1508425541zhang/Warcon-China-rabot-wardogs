<script lang="ts">
	import { onMount } from 'svelte';
	import { refreshVisible } from '$lib/refresh-visible';
	onMount(() => refreshVisible(invalidateAll));
	import { integrityMetricDisplay } from '$lib/integrity-metric-display';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage, rconPost } from '$lib/api';
	import { fmtNum, fmtTime } from '$lib/format';
	import { causeLabel } from '$lib/causes';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import BanDialog from '$lib/components/BanDialog.svelte';
	import CareerPanel from '$lib/components/CareerPanel.svelte';
	import CombatSummary from '$lib/components/CombatSummary.svelte';
	import { describeSync, STATE_TONE } from '$lib/lists';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort } from '$lib/table.svelte';
	import type { DossierView, ListSyncServer, ListSyncSummary } from '$lib/types';
	import type { PageProps } from './$types';
	import { integrityPartText, legacyRiskReasonZh } from '$lib/integrity-display';

	let { data }: PageProps = $props();
	let d = $derived<DossierView>(data.dossier);
	let id = $derived(data.server.id);
	let moderate = $derived(can(data.server.caps, 'players.moderate'));
	let chat = $derived(can(data.server.caps, 'chat.send'));
	let bans = $derived(can(data.server.caps, 'bans.manage'));
	let notes = $derived(can(data.server.caps, 'players.notes'));
	let base = $derived(`/api/servers/${encodeURIComponent(id)}/players/${d.steamId}`);
	let onThisServer = $derived(d.online?.serverId === id);
	let orgListsPath = $derived(`/api/orgs/${encodeURIComponent(data.server.orgId)}/lists`);

	let busy = $state(false);
	let banning = $state(false);

	const serverSort = new TableSort<DossierView['perServer'][number]>({
		server: { by: (s) => s.serverName },
		sessions: { by: (s) => s.sessions, dir: 'desc' },
		minutes: { by: (s) => s.minutes, dir: 'desc' },
		kills: { by: (s) => s.kills, dir: 'desc' },
		deaths: { by: (s) => s.deaths, dir: 'desc' },
		lastSeen: { by: (s) => s.lastSeen, dir: 'desc' }
	});
	let perServer = $derived(serverSort.sorted(d.perServer));
	const sessionSort = new TableSort<DossierView['recent'][number]>({
		joined: { by: (s) => s.joinedAt, dir: 'desc' },
		server: { by: (s) => s.serverName },
		name: { by: (s) => s.name },
		faction: { by: (s) => s.faction },
		minutes: { by: (s) => s.minutes, dir: 'desc' },
		seeded: { by: (s) => s.seedMinutes, dir: 'desc' },
		kills: { by: (s) => s.kills, dir: 'desc' },
		deaths: { by: (s) => s.deaths, dir: 'desc' },
		cash: { by: (s) => s.cash, dir: 'desc' }
	});
	let recent = $derived(sessionSort.sorted(d.recent));
	const clock = (iso: string) => new Date(iso).toLocaleTimeString(undefined, { hour12: false });

	/** remove the player from an org list (unban across the org, or withdraw the reserved slot) */
	async function orgRemove(kind: 'ban' | 'reserve') {
		const what =
			kind === 'ban'
				? `在 ${data.server.orgName} 的所有服务器解除 ${d.name} 的封禁？面板会撤销此前应用的封禁。`
				: `在 ${data.server.orgName} 的所有服务器撤回 ${d.name} 的预留位？`;
		if (
			!(await confirmDialog(what, { okLabel: kind === 'ban' ? '解除封禁' : '撤回', danger: true }))
		)
			return;
		await run(async () => {
			const r = await api<{ sync: ListSyncSummary }>(
				'DELETE',
				`${orgListsPath}/${kind}/entries/${d.steamId}`
			);
			toast(describeSync(r.sync, kind === 'ban' ? '已解除封禁。' : '已撤回预留位。'), 'ok', 8000);
		}, '');
	}
	const orgReserve = () =>
		run(async () => {
			const r = await api<{ sync: ListSyncSummary }>('POST', `${orgListsPath}/reserve/entries`, {
				steamId: d.steamId,
				reason: d.name
			});
			toast(describeSync(r.sync, '预留位已分配。'), 'ok', 8000);
		}, '');
	let note = $state('');
	let watchReason = $state('');
	let whisper = $state('');
	let reason = $state('');
	$effect(() => {
		watchReason = d.watch.reason;
	});

	async function run(fn: () => Promise<unknown>, done: string) {
		busy = true;
		try {
			await fn();
			if (done) toast(done, 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	const addNote = () => {
		const body = note.trim();
		if (!body) return;
		void run(async () => {
			await api('POST', `${base}/notes`, { body });
			note = '';
		}, '备注已添加。');
	};
	async function deleteNote(noteId: number) {
		if (!(await confirmDialog('删除这条备注？', { okLabel: '删除', danger: true }))) return;
		await run(() => api('DELETE', `${base}/notes/${noteId}`), '备注已删除。');
	}
	const setWatch = (watched: boolean) =>
		run(
			() => api('PUT', `${base}/watch`, { watched, reason: watchReason.trim() }),
			watched ? '已加入关注名单。' : '已从关注名单移除。'
		);
	const refreshSteam = () => run(() => api('POST', `${base}/steam`), 'Steam 数据已刷新。');

	async function act(action: string, params: object, confirm?: string) {
		if (confirm && !(await confirmDialog(confirm, { okLabel: '确认执行', danger: true }))) return;
		await run(async () => {
			const r = await rconPost<{ message?: string }>(id, action, params);
			toast(r?.message || '操作已完成。', 'ok');
		}, '');
	}

	// A ban goes on the server's own list, so the panel keeps the reason and who placed it.
	async function banHere() {
		const sure = await confirmDialog(`在 ${data.server.name} 封禁 ${d.name}（${d.steamId}）？`, {
			okLabel: '确认封禁',
			danger: true
		});
		if (!sure) return;
		await run(async () => {
			const res = await api<{ sync: ListSyncServer }>(
				'POST',
				`/api/servers/${encodeURIComponent(id)}/lists/ban/entries`,
				{ steamId: d.steamId, reason: reason.trim() }
			);
			toast(describeSync({ servers: [res.sync] }, `已封禁 ${d.name}。`), 'ok', 8000);
		}, '');
	}

	const SCOREBOARD_NOTE = '游戏计分板数据，按玩家各场次汇总。';
	const minutes = (m: number) => (m >= 90 ? `${(m / 60).toFixed(1)} 小时` : `${m} 分钟`);
	const kd = (k: number, dd: number) => (dd ? (k / dd).toFixed(2) : k ? `${k}.00` : '—');
	const integrityLevelName = (value: string | null) =>
		value
			? ((
					{
						NORMAL: '正常',
						PASSIVE_WATCH: '被动观察',
						ACTIVE_WATCH: '主动观察',
						AUTO_KO: '达到移出阈值',
						AUTO_QUARANTINE_ELIGIBLE: '达到隔离资格阈值'
					} as Record<string, string>
				)[value] ?? value)
			: '未评分';
	let integrityTiles = $derived.by(() => {
		const i = data.integrity;
		if (!i) return [] as [string, string | number][];
		return [
			['当前击杀', i.current?.kills ?? '—'],
			['当前死亡', i.current?.deaths ?? '—'],
			[
				'当前 KD',
				i.current
					? i.current.deaths
						? kd(i.current.kills, i.current.deaths)
						: i.current.kills
							? '∞'
							: '—'
					: '—'
			],
			[
				'实时步兵 KPM（最近 60 秒）',
				integrityMetricDisplay(
					i.metrics?.kpm60 ?? 0,
					i.metricsAvailable,
					i.metrics?.reliable60 !== false
				)
			],
			[
				'60 秒有效步兵击杀',
				integrityMetricDisplay(
					i.metrics?.infantryKills60 ?? 0,
					i.metricsAvailable,
					i.metrics?.reliable60 !== false,
					0
				)
			],
			[
				'180 秒有效步兵击杀',
				integrityMetricDisplay(
					i.metrics?.infantryKills180 ?? 0,
					i.metricsAvailable,
					i.metrics?.reliable180 !== false,
					0
				)
			],
			[
				'步兵 KPM180（≥ 为已确认下限）',
				integrityMetricDisplay(
					i.metrics?.kpm180 ?? 0,
					i.metricsAvailable,
					i.metrics?.reliable180 !== false
				)
			],
			[
				'近 10 分钟峰值 KPM（≥ 为已确认下限）',
				integrityMetricDisplay(
					i.metrics?.peakKpm180 ?? 0,
					i.metricsAvailable,
					i.metrics?.reliable !== false
				)
			],
			[
				'独立受害者（≥ 为已确认下限）',
				integrityMetricDisplay(
					i.metrics?.uniqueVictims180 ?? 0,
					i.metricsAvailable,
					i.metrics?.reliable !== false,
					0
				)
			],
			['风险分', i.riskScore ?? '—'],
			['风险级别', integrityLevelName(i.riskLevel)]
		] as [string, string | number][];
	});
	const integrityParts = (value: unknown): { code: string; points: number; detail: string }[] =>
		Array.isArray(value)
			? value.filter(
					(part): part is { code: string; points: number; detail: string } =>
						!!part && typeof part.code === 'string' && typeof part.points === 'number'
				)
			: [];
	const RISK_TONE = { low: 'ok', medium: 'warn', high: 'err' } as const;
	const legacyLevelName = { low: '低', medium: '中', high: '高' } as const;
	const ACTION_LABEL: Record<string, string> = {
		'rcon.kick': '踢出',
		'rcon.ban': '封禁',
		'rcon.unban': '解除封禁',
		'rcon.kill': '击杀',
		'rcon.whisper': '私聊',
		'rcon.changeTeam': '换队',
		'player.note': '备注',
		'player.note.delete': '删除备注',
		'player.watch': '观察名单'
	};
</script>

<div class="mb-4 flex flex-wrap items-center gap-3">
	<div class="min-w-0">
		<a
			href="/server/{encodeURIComponent(id)}/players"
			class="caps text-mist-400 hover:text-mist-100">← 玩家</a
		>
		<h2 class="flex flex-wrap items-center gap-2 text-xl font-semibold tracking-tight">
			{#if d.steam?.avatar}<img
					src={d.steam.avatar}
					alt=""
					class="h-8 w-8 rounded-[2px] border border-black"
					referrerpolicy="no-referrer"
				/>{/if}
			<span class="truncate">{d.name}</span>
			{#if d.online}<Badge tone="ok">在线 · {onThisServer ? '本服务器' : d.online.serverName}</Badge
				>{/if}
			{#if d.watch.watched}<Badge tone="warn">观察名单</Badge>{/if}
			<Badge tone={RISK_TONE[d.risk.level]}
				>入服账号风险 {legacyLevelName[d.risk.level]} · {d.risk.score}</Badge
			>
		</h2>
		<div class="mt-1 flex flex-wrap items-center gap-2 text-[12.5px] text-mist-400">
			<span class="font-mono">{d.steamId}</span>
			{#if d.steam?.profileUrl}<a
					href={d.steam.profileUrl}
					target="_blank"
					rel="noopener noreferrer"
					class="text-accent hover:underline">Steam 资料 ↗</a
				>{/if}
			{#if d.names.length > 1}<span
					>· 曾用名 {d.names.slice(1, 6).join(', ')}{d.names.length > 6 ? '…' : ''}</span
				>{/if}
		</div>
	</div>
</div>

<div class="mb-4 grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
	{#each [['场次', fmtNum(d.summary.sessions), '每次进入至离开服务器计为一场。'], ['游戏时间', d.summary.sessions ? minutes(d.summary.minutes) : '—', ''], ['击杀', fmtNum(d.summary.kills), SCOREBOARD_NOTE], ['死亡', fmtNum(d.summary.deaths), SCOREBOARD_NOTE], ['KD', kd(d.summary.kills, d.summary.deaths), SCOREBOARD_NOTE], ['首次出现', d.summary.firstSeen ? fmtTime(d.summary.firstSeen) : '—', '']] as [label, value, note] (label)}
		<div class="panel py-4" title={note || undefined}>
			<div class="caps text-mist-400">{label}</div>
			<div class="mt-1 font-display text-2xl font-semibold tabular">{value}</div>
		</div>
	{/each}
</div>

{#if data.integrity}
	<section class="mb-4 panel p-4">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<h3 class="text-base font-semibold text-white">社区风控</h3>
			<a class="text-sm text-accent" href="/server/{id}/integrity">查看风控总览 →</a>
		</div>
		<p class="mt-1 text-xs text-mist-400">
			KPM 与风险分独立显示：180 秒 KPM＝有效步兵击杀数 ÷ 3；实时 KPM＝最近 60
			秒有效步兵击杀数。回传可用但没有击杀时显示 0.00；≥ 为已确认下限，“—”为数据不可用。KD
			不单独加分，未评分表示最近没有风险评分记录。
		</p>
		<div class="mt-3 grid grid-cols-2 gap-3 md:grid-cols-4 xl:grid-cols-8">
			{#each integrityTiles as item (item[0])}
				<div class="rounded-ctl border border-white/8 p-3">
					<div class="text-xs text-mist-400">{item[0]}</div>
					<div class="mt-1 font-mono text-lg text-white">{item[1]}</div>
				</div>
			{/each}
		</div>
		<div class="mt-3 flex flex-wrap gap-4 text-xs text-mist-400">
			<span>过去 24 小时独立举报人：{data.integrity.reports24h}</span>
			{#if data.integrity.latestWindow}<span
					>最近异常窗口：{data.integrity.latestWindow.kpm180.toFixed(2)} KPM · {fmtTime(
						data.integrity.latestWindow.observedAt
					)}</span
				>{/if}
			{#if data.integrity.firstSeen}<span>首次出现：{fmtTime(data.integrity.firstSeen)}</span>{/if}
			{#if data.integrity.lastSeen}<span>最近出现：{fmtTime(data.integrity.lastSeen)}</span>{/if}
		</div>
		{#if data.integrity.aliases.length > 1}<p class="mt-2 text-xs text-mist-400">
				历史昵称：{data.integrity.aliases.join('、')}
			</p>{/if}
		{#if integrityParts(data.integrity.breakdown).length}<details class="mt-3 text-sm">
				<summary class="cursor-pointer text-accent">展开风险分项</summary>
				<ul class="mt-2 space-y-1 text-xs text-mist-300">
					{#each integrityParts(data.integrity.breakdown) as part (part.code)}<li>
							+{part.points}
							{integrityPartText(part.code, part.detail)}
						</li>{/each}
				</ul>
			</details>{/if}
	</section>
{/if}

<div class="grid grid-cols-1 gap-4 xl:grid-cols-[3fr_2fr]">
	<div class="space-y-4">
		{#if d.bannedOn.length}
			<div class="callout border-l-danger">
				<b>该玩家已在本组织 {d.bannedOn.length} / {d.orgServerCount} 台服务器被封禁。</b>
				{#each d.bannedOn as b (b.serverId)}
					<div>
						{b.serverName}{#if b.reason}: {b.reason}{/if}{#if b.bannedBy}
							<span class="text-mist-400">（操作人：{b.bannedBy}）</span>{/if}
					</div>
				{/each}
			</div>
		{/if}

		<div class="panel">
			<span class="label-sm">按服务器</span>
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							<SortHeader sort={serverSort} key="server">服务器</SortHeader>
							<SortHeader sort={serverSort} key="sessions" num>场次</SortHeader>
							<SortHeader sort={serverSort} key="minutes" num>游戏时间</SortHeader>
							<SortHeader sort={serverSort} key="kills" num>K</SortHeader>
							<SortHeader sort={serverSort} key="deaths" num>D</SortHeader>
							<SortHeader sort={serverSort} key="lastSeen">最近出现</SortHeader>
						</tr>
					</thead>
					<tbody>
						{#each perServer as s (s.serverId)}
							<tr>
								<td
									><a
										href="/server/{encodeURIComponent(s.serverId)}/players/{d.steamId}"
										class="hover:text-accent hover:underline">{s.serverName}</a
									></td
								>
								<td class="num">{s.sessions}</td><td class="num">{minutes(s.minutes)}</td>
								<td class="num">{fmtNum(s.kills)}</td><td class="num">{fmtNum(s.deaths)}</td>
								<td class="whitespace-nowrap text-mist-400">{fmtTime(s.lastSeen)}</td>
							</tr>
						{:else}
							<tr
								><td colspan="6" class="py-6 text-center text-mist-600"
									>在你有权访问的服务器上未见过该玩家。</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			</div>
		</div>

		<div class="panel">
			<span class="label-sm">近期场次</span>
			<div class="max-h-[420px] table-wrap">
				<table>
					<thead>
						<tr>
							<SortHeader sort={sessionSort} key="joined">加入时间</SortHeader>
							<SortHeader sort={sessionSort} key="server">服务器</SortHeader>
							<SortHeader sort={sessionSort} key="name">名称</SortHeader>
							<SortHeader sort={sessionSort} key="faction">阵营</SortHeader>
							<SortHeader sort={sessionSort} key="minutes" num>时长</SortHeader>
							<SortHeader sort={sessionSort} key="seeded" num>种子服时间</SortHeader>
							<SortHeader sort={sessionSort} key="kills" num>K</SortHeader>
							<SortHeader sort={sessionSort} key="deaths" num>D</SortHeader>
							<SortHeader sort={sessionSort} key="cash" num>现金</SortHeader>
						</tr>
					</thead>
					<tbody>
						{#each recent as s (s.id)}
							<tr>
								<td class="whitespace-nowrap">{fmtTime(s.joinedAt)}</td>
								<td>{s.serverName}</td>
								<td>{s.name}</td>
								<td>{s.faction || '—'}</td>
								<td class="num"
									>{minutes(s.minutes)}{#if !s.leftAt}<Badge tone="ok" class="ml-1">实时</Badge
										>{/if}</td
								>
								<td class="num">{s.seedMinutes ? minutes(s.seedMinutes) : '—'}</td>
								<td class="num">{s.kills}</td><td class="num">{s.deaths}</td>
								<td class="num">{fmtNum(s.cash)}</td>
							</tr>
						{:else}
							<tr><td colspan="9" class="py-6 text-center text-mist-600">暂无场次。</td></tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>

		{#if d.combat}
			<div class="panel">
				<span class="label-sm">战斗数据</span>
				<p class="mb-3 text-[12.5px] text-mist-600">
					数据来自你有权查看的组织服务器击杀事件。误杀队友计为击杀，自杀计为死亡；击杀事件只覆盖接入后的时间，因此可能与上方计分板汇总不同。
					<a
						href="/server/{encodeURIComponent(data.server.id)}/kills?player={encodeURIComponent(
							d.steamId
						)}"
						class="text-accent hover:underline">查看本服全部击杀与死亡 →</a
					>
				</p>
				<CombatSummary
					combat={d.combat}
					hrefFor={(steamId) => `/server/${encodeURIComponent(id)}/players/${steamId}`}
				/>
				{#if d.combat.recent.length}
					<span class="mt-4 field-label">近期击杀与死亡</span>
					<div class="max-h-[320px] table-wrap">
						<table>
							<thead
								><tr
									><th>时间</th><th>服务器</th><th>击杀者</th><th>受害者</th><th>击杀原因</th><th
										class="num">距离</th
									><th></th></tr
								></thead
							>
							<tbody>
								{#each d.combat.recent as k (k.eventId)}
									<tr class={k.teamKill ? 'text-warn' : ''}>
										<td class="whitespace-nowrap text-mist-400" title={fmtTime(k.ts)}
											>{clock(k.ts)}</td
										>
										<td>{k.serverName}</td>
										<td class={k.killer?.steamId === d.steamId ? 'font-semibold' : ''}
											>{k.killer?.name ?? '—'}</td
										>
										<td class={k.victim.steamId === d.steamId ? 'font-semibold' : ''}
											>{k.victim.name}</td
										>
										<td>{causeLabel(k.cause) || (k.tags.includes('Falling') ? '坠落' : '—')}</td>
										<td class="num"
											>{k.distanceM === null ? '—' : `${Math.round(k.distanceM)} m`}</td
										>
										<td class="whitespace-nowrap">
											{#if k.teamKill}<span class="chip">误杀队友</span>{/if}
											{#if k.suicide}<span class="chip">自杀</span>{/if}
											{#if k.headshot}<span class="chip">爆头</span>{/if}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		{/if}

		<div class="panel">
			<div class="mb-3 flex items-center gap-2">
				<span class="label-sm mb-0!">生涯数据</span>
				<a
					href="/server/{encodeURIComponent(id)}/leaderboard"
					class="ml-auto text-[12px] text-accent hover:underline">查看排行榜 →</a
				>
			</div>
			<CareerPanel
				career={data.career}
				serverName={data.server.name}
				orgName={data.server.orgName}
				multiServer={data.multiServer}
				matchHref={(m) => `/server/${encodeURIComponent(m.serverId)}/matches/${m.matchId}`}
			/>
		</div>

		<div class="panel">
			<span class="label-sm">对该玩家的管理操作</span>
			<div class="max-h-[360px] table-wrap">
				<table>
					<thead
						><tr><th>时间</th><th>操作人</th><th>操作</th><th>服务器</th><th>结果</th></tr></thead
					>
					<tbody>
						{#each d.actions as a (a.id)}
							<tr>
								<td class="whitespace-nowrap">{fmtTime(a.ts)}</td>
								<td>{a.actorName || '—'}</td>
								<td><span class="chip">{ACTION_LABEL[a.action] || a.action}</span></td>
								<td>{a.serverName}</td>
								<td class="max-w-[360px]">
									<span class={a.outcome === 'ok' ? '' : 'text-danger'}
										>{a.message || a.outcome}</span
									>
								</td>
							</tr>
						{:else}
							<tr
								><td colspan="5" class="py-6 text-center text-mist-600"
									>你可查看的操作记录为空；对该玩家的操作会显示在这里。</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	</div>

	<div class="space-y-4 self-start">
		{#if onThisServer && (moderate || chat)}
			<div class="panel border-accent/40">
				<span class="label-sm">快捷操作（玩家在本服在线）</span>
				<div class="join w-full">
					<input
						class="input"
						type="text"
						placeholder="私聊消息…"
						maxlength="200"
						bind:value={whisper}
					/>
					<button
						class="btn btn-primary"
						disabled={busy || !chat || !whisper.trim()}
						onclick={async () => {
							await act('whisper', { steamId: d.steamId, message: whisper.trim() });
							whisper = '';
						}}>私聊</button
					>
				</div>
				<div class="join join-wrap mt-2 w-full">
					<input
						class="input"
						type="text"
						placeholder="原因（可选）…"
						maxlength="200"
						bind:value={reason}
					/>
					<button
						class="btn btn-danger"
						disabled={busy || !moderate}
						onclick={() =>
							act('kick', { steamId: d.steamId, reason: reason.trim() }, `踢出 ${d.name}？`)}
						>踢出</button
					>
					<button class="btn btn-danger" disabled={busy || !bans} onclick={banHere}>封禁</button>
				</div>
			</div>
		{/if}

		<!-- an entry, its reason and who added it are for those who may edit that list -->
		{#if d.orgLists.canBan || d.orgLists.canReserve}
			<div class="panel">
				<div class="mb-3 flex items-center gap-2">
					<span class="label-sm mb-0!">组织名单</span>
					<a
						href="/orgs/{encodeURIComponent(data.server.orgId)}/{d.orgLists.canBan
							? 'bans'
							: 'reserved'}"
						class="ml-auto text-[12px] text-accent hover:underline">打开名单 →</a
					>
				</div>
				<div class="space-y-3 text-[13px]">
					{#if d.orgLists.canBan}
						<div class="flex flex-wrap items-center gap-2">
							{#if d.orgLists.ban}
								{@const b = d.orgLists.ban}
								<Badge tone="err">已在全组织封禁</Badge>
								<span class="min-w-0 flex-1 truncate text-mist-400"
									>{b.reason || '未填写原因'} · 操作人：{b.addedByName || '—'}{#if b.expiresAt}
										· 截止 {fmtTime(b.expiresAt)}{/if}</span
								>
								<span class="inline-flex flex-wrap gap-1">
									{#each b.servers as s (s.serverId)}
										<span title="{s.serverName}: {s.state}{s.error ? ` — ${s.error}` : ''}"
											><Badge tone={STATE_TONE[s.state]}>{s.serverName}</Badge></span
										>
									{/each}
								</span>
								<button class="btn btn-sm" disabled={busy} onclick={() => orgRemove('ban')}
									>在全组织解除封禁</button
								>
							{:else}
								<span class="text-mist-400">不在组织封禁名单中。</span>
								<button
									class="ml-auto btn btn-sm btn-danger"
									disabled={busy}
									onclick={() => (banning = true)}>在全组织封禁</button
								>
							{/if}
						</div>
					{/if}
					{#if d.orgLists.canReserve}
						<div class="flex flex-wrap items-center gap-2">
							{#if d.orgLists.reserve}
								{@const r = d.orgLists.reserve}
								<Badge tone="accent">预留位</Badge>
								<span class="min-w-0 flex-1 truncate text-mist-400"
									>{r.reason || '全组织'}{#if r.member}
										· 成员{/if}{#if r.expiresAt}
										· 截止 {fmtTime(r.expiresAt)}{/if}</span
								>
								<span class="inline-flex flex-wrap gap-1">
									{#each r.servers as s (s.serverId)}
										<span title="{s.serverName}: {s.state}{s.error ? ` — ${s.error}` : ''}"
											><Badge tone={STATE_TONE[s.state]}>{s.serverName}</Badge></span
										>
									{/each}
								</span>
								{#if !r.member}
									<button class="btn btn-sm" disabled={busy} onclick={() => orgRemove('reserve')}
										>撤回</button
									>
								{/if}
							{:else}
								<span class="text-mist-400">组织未分配预留位。</span>
								<button class="ml-auto btn btn-sm" disabled={busy} onclick={orgReserve}
									>预留一个位置</button
								>
							{/if}
						</div>
					{/if}
				</div>
			</div>
		{/if}

		<div class="panel">
			<div class="mb-3 flex items-center gap-2">
				<span class="label-sm mb-0!">入服账号风险</span>
				<Badge tone={RISK_TONE[d.risk.level]} class="ml-auto"
					>{legacyLevelName[d.risk.level]} · {d.risk.score}</Badge
				>
			</div>
			{#if d.risk.reasons.length}
				<ul class="space-y-1 text-[13px]">
					{#each d.risk.reasons as r (r.code + r.text)}
						<li class="flex gap-2">
							<span class="font-mono text-[12px] text-mist-600 tabular">+{r.weight}</span>
							<span>{legacyRiskReasonZh(r.code, r.text)}</span>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="text-[13px] text-mist-400">暂无异常。</p>
			{/if}
			<p class="note">
				这是 Warcon 原有的参考分，依据 Steam
				公开资料、已记录的游戏统计、组织封禁名单和观察名单计算；无法读取玩家瞄准、位置或输入数据。
				{#if !d.steamEnabled}<span class="text-warn"
						>未设置 STEAM_API_KEY，无法获取 Steam 账号年龄与 VAC 状态。</span
					>{/if}
			</p>
		</div>

		<div class="panel">
			<div class="mb-3 flex items-center gap-2">
				<span class="label-sm mb-0!">Steam</span>
				{#if d.steamEnabled}
					<button class="ml-auto btn btn-sm" onclick={refreshSteam} disabled={busy}>刷新</button>
				{/if}
			</div>
			{#if d.steam}
				{#if d.steam.error}<p class="mb-2 text-[13px] text-warn">{d.steam.error}</p>{/if}
				<div class="kv">
					<span class="text-mist-400">昵称</span><span>{d.steam.persona || '—'}</span>
				</div>
				<div class="kv">
					<span class="text-mist-400">账号年龄</span>
					<span
						>{#if d.steam.accountAgeDays === null}未知（{d.steam.public
								? '无创建日期'
								: '私人资料'}）{:else}{d.steam.accountAgeDays} 天 · 创建于 {fmtTime(
								d.steam.accountCreatedAt
							).slice(0, 12)}{/if}</span
					>
				</div>
				<div class="kv">
					<span class="text-mist-400">VAC 封禁</span>
					<span class={d.steam.vacBans ? 'text-danger' : ''}
						>{d.steam.vacBans}{#if d.steam.vacBans && d.steam.daysSinceLastBan !== null}
							· 最近一次在 {d.steam.daysSinceLastBan} 天前{/if}</span
					>
				</div>
				<div class="kv">
					<span class="text-mist-400">游戏封禁</span>
					<span class={d.steam.gameBans ? 'text-danger' : ''}>{d.steam.gameBans}</span>
				</div>
				<div class="kv">
					<span class="text-mist-400">Steam 好友</span>
					<span>
						{#if d.steam.friendsState === 'private'}好友列表未公开
						{:else if d.steam.friendsState === 'unknown'}无法获取
						{:else}已检查 {d.steam.friendsChecked} 位好友，其中 {d.steam.bannedFriends} 位有封禁记录{#if d.steam.friendsState === 'partial'}
								（总计 {d.steam.friendsTotal} 位）{/if}{/if}
					</span>
				</div>
				{#if d.steam.communityBanned || d.steam.economyBan !== 'none'}
					<div class="kv">
						<span class="text-mist-400">其他</span>
						<span class="text-warn"
							>{[
								d.steam.communityBanned ? '社区封禁' : '',
								d.steam.economyBan !== 'none' ? `交易限制：${d.steam.economyBan}` : ''
							]
								.filter(Boolean)
								.join(', ')}</span
						>
					</div>
				{/if}
				<p class="note">获取时间：{fmtTime(d.steam.fetchedAt)}。</p>
			{:else if d.steamEnabled}
				<p class="text-[13px] text-mist-400">
					尚未查询。<button class="text-accent underline" onclick={refreshSteam}>立即获取</button>。
				</p>
			{:else}
				<p class="text-[13px] text-mist-400">
					设置 <code class="font-mono">STEAM_API_KEY</code> 后可查看昵称、账号年龄和封禁记录。
				</p>
			{/if}
		</div>

		<div class="panel {d.watch.watched ? 'border-warn/50' : ''}">
			<span class="label-sm">观察名单</span>
			{#if d.watch.watched}
				<p class="mb-2 text-[13px]">
					已加入观察名单{#if d.watch.reason}：<b>{d.watch.reason}</b>{/if}。
					{#if notes}
						<span class="text-mist-400"
							>添加人：{d.watch.updatedByName || '?'} · {fmtTime(d.watch.updatedAt)}</span
						>
					{/if}
				</p>
				<button class="btn btn-sm" disabled={busy || !notes} onclick={() => setWatch(false)}
					>移出观察名单</button
				>
			{:else}
				<div class="join w-full">
					<input
						class="input"
						type="text"
						placeholder="原因（所有管理员可见）…"
						maxlength="300"
						bind:value={watchReason}
					/>
					<button class="btn" disabled={busy || !notes} onclick={() => setWatch(true)}>观察</button>
				</div>
			{/if}
			<p class="note">
				该名单在 {data.server.orgName} 的所有服务器共享。观察中的玩家会在玩家表中标记，也可作为自动化规则条件。
			</p>
		</div>

		<!-- notes are read by those who may write them -->
		{#if notes}
			<div class="panel">
				<span class="label-sm">备注</span>
				{#if notes}
					<div class="mb-3">
						<textarea
							class="min-h-[70px] input"
							placeholder="下一位管理员需要了解的情况…"
							maxlength="2000"
							bind:value={note}></textarea>
						<div class="mt-2 flex justify-end">
							<button
								class="btn btn-sm btn-primary"
								disabled={busy || !note.trim()}
								onclick={addNote}>添加备注</button
							>
						</div>
					</div>
				{/if}
				<div class="space-y-2">
					{#each d.notes as n (n.id)}
						<div class="rounded-ctl border border-black bg-ink-950 px-3 py-2">
							<div class="mb-1 flex items-center gap-2 text-[12px] text-mist-400">
								<b class="text-mist-100">{n.authorName || '—'}</b>
								<span>{fmtTime(n.createdAt)}</span>
								{#if n.deletable}<button
										class="ml-auto btn btn-sm btn-ghost"
										aria-label="删除备注"
										disabled={busy}
										onclick={() => deleteNote(n.id)}>✕</button
									>{/if}
							</div>
							<div class="text-[13.5px] whitespace-pre-wrap">{n.body}</div>
						</div>
					{:else}
						<p class="text-[13px] text-mist-600">暂无备注。</p>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</div>

{#if banning}
	<BanDialog
		orgId={data.server.orgId}
		orgName={data.server.orgName}
		steamId={d.steamId}
		name={d.name}
		canOrg
		onclose={() => (banning = false)}
		ondone={() => invalidateAll()}
	/>
{/if}
