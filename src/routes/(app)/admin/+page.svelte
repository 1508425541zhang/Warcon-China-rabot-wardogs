<script lang="ts">
	import { api, errorMessage } from '$lib/api';
	import { poll } from '$lib/poll';
	import { fmtAgo, fmtBytes, fmtCompact, fmtNum, fmtSpan } from '$lib/format';
	import { fmtRate, meanOf, perSecond } from '$lib/rates';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	type Overview = typeof data.overview;

	// svelte-ignore state_referenced_locally
	let live = $state<Overview>(data.overview);
	let prev = $state<Overview | null>(null);
	let recounting = $state(false);

	async function refresh(recount = false) {
		try {
			const next = (
				await api<{ overview: Overview }>(
					'GET',
					`/api/admin/overview${recount ? '?recount=1' : ''}`
				)
			).overview;
			prev = live;
			live = next;
		} catch (err) {
			if (recount) toast(errorMessage(err), 'err');
			/* otherwise keep the last view */
		}
	}
	$effect(() => poll(() => refresh(), 5000));
	async function recount() {
		recounting = true;
		try {
			await refresh(true);
		} finally {
			recounting = false;
		}
	}

	// Rates between the last two readings: the worker's counters on the worker's clock, the web's on its own.
	const worker = $derived(live.worker);
	const wp = $derived(live.worker?.process);
	const wpPrev = $derived(prev?.worker?.process);
	const web = $derived(live.web);
	const webPrev = $derived(prev?.web);
	const wRate = (pick: (p: NonNullable<typeof wp>) => number) =>
		wp ? perSecond(wpPrev ? pick(wpPrev) : undefined, pick(wp), wpPrev?.at, wp.at) : null;
	const webRate = (pick: (p: typeof web) => number) =>
		perSecond(webPrev ? pick(webPrev) : undefined, pick(web), webPrev?.at, web.at);

	const obsPerSec = $derived(wRate((p) => p.observations.ok + p.observations.failed));
	const obsMean = $derived(
		wp
			? meanOf(
					wpPrev?.observations.seconds,
					wp.observations.seconds,
					wpPrev ? wpPrev.observations.ok + wpPrev.observations.failed : undefined,
					wp.observations.ok + wp.observations.failed
				)
			: null
	);
	const finished = (w: NonNullable<Overview['worker']>) =>
		w.delivery.delivered + w.delivery.failed + w.delivery.skipped + w.delivery.unknown;
	const deliveriesPerMin = $derived.by(() => {
		const p = prev?.worker;
		if (!worker || !p) return null;
		const r = perSecond(finished(p), finished(worker), p.process.at, worker.process.at);
		return r === null ? null : r * 60;
	});
	const killsPerSec = $derived(webRate((p) => p.feed.kills));
	const postsPerSec = $derived(webRate((p) => p.feed.posts));
	const skippedPerSec = $derived(webRate((p) => p.feed.skipped));
	const dupesPerSec = $derived(webRate((p) => p.feed.duplicates));
	const reqPerSec = $derived(webRate((p) => p.requests.total));
	const publicPerSec = $derived(webRate((p) => p.requests.public));
	const reqMean = $derived(
		meanOf(
			webPrev?.requests.seconds,
			web.requests.seconds,
			webPrev?.requests.total,
			web.requests.total
		)
	);

	const build = (b: typeof web.build) => (b.commit ? `${b.version} (${b.commit})` : b.version);
	/** mid-deploy, or one host left on an old image */
	const buildsDiffer = $derived(!!wp && build(wp.build) !== build(web.build));

	const players = $derived(worker ? worker.players : live.fleet.players);
	const unreachable = $derived(live.fleet.servers - live.fleet.serversOk);
	const unobserved = $derived(live.fleet.servers - live.fleet.serversObserved);
	const ms = (s: number | null) => (s === null ? '…' : `${Math.round(s * 1000)} ms`);
	/** sub-minute spans to a tenth: the beat is a quarter second, the oldest pending row a few */
	const secs = (v: number) => (v < 60_000 ? `${Math.round(v / 100) / 10} s` : fmtSpan(v));
	const lag = (s: number | null) => (s === null ? '' : ` · lag ${Math.round(s * 1000)} ms`);

	const TOP_TABLES = 8;
	const shown = $derived(live.database.tables.slice(0, TOP_TABLES));
	const rest = $derived(live.database.tables.slice(TOP_TABLES).reduce((a, t) => a + t.bytes, 0));
	const share = (bytes: number) =>
		live.database.bytes ? Math.min(100, (bytes / live.database.bytes) * 100) : 0;

	const TILE =
		'min-w-0 rounded-card border border-black bg-ink-900 px-4 py-3.5 shadow-[inset_0_1px_0_rgb(255_255_255/0.04)]';
	const BIG =
		'mt-1.5 font-display text-[34px] leading-none font-semibold tracking-[0.04em] tabular';
	const SMALL = 'text-[18px] font-medium tracking-[0.02em] text-mist-400';
	const SUB = 'mt-1.5 text-[12px] text-mist-400';
</script>

<svelte:head><title>概览 · 站点管理 · {data.appName}</title></svelte:head>

<div class="mb-4 flex flex-wrap items-center gap-2">
	{#if !worker}<Badge tone="err">无法连接后台进程</Badge>{:else if worker.owner}<Badge tone="ok"
			>后台进程持有租约</Badge
		>{:else if worker.enabled}<Badge tone="warn">另一个进程持有租约</Badge>{:else}<Badge tone="err"
			>后台进程未运行</Badge
		>{/if}
	{#if worker?.behind}<Badge tone="err">{worker.behind} 滞后</Badge>{/if}
	{#if worker?.stuck}<Badge tone="err">{worker.stuck} 卡住</Badge>{/if}
	{#if buildsDiffer}<Badge tone="warn">网页与后台进程版本不同</Badge>{/if}
	{#if !live.metricsOn}<Badge tone="warn">指标未启用：请设置 METRICS_TOKEN</Badge>{/if}
	<span class="ml-auto text-[12px] text-mist-600">已更新 {fmtAgo(live.at)} · 每 5 秒刷新</span>
</div>

<div class="mb-3 grid grid-cols-2 gap-3 lg:grid-cols-6">
	<div class={TILE}>
		<span class="caps text-mist-400">在线玩家</span>
		<div class={BIG}>{fmtNum(players)}</div>
		<div class={SUB}>分布于 {fmtNum(live.fleet.serversOk)} 台服务器</div>
	</div>
	<div class={TILE}>
		<span class="caps text-mist-400">服务器</span>
		<div class={BIG}>
			{fmtNum(live.fleet.serversOk)}<small class={SMALL}> / {fmtNum(live.fleet.servers)}</small>
		</div>
		<div class={SUB}>
			{fmtNum(unreachable)} 无法连接{#if unobserved > 0}
				· {fmtNum(unobserved)} 尚未观测到{/if}
		</div>
	</div>
	<div class={TILE}>
		<span class="caps text-mist-400">组织</span>
		<div class={BIG}>{fmtNum(live.fleet.orgs)}</div>
		<div class={SUB}>{fmtNum(live.fleet.orgsWeek)} 过去 7 天加入</div>
	</div>
	<div class={TILE}>
		<span class="caps text-mist-400">用户</span>
		<div class={BIG}>{fmtNum(live.fleet.users)}</div>
		<div class={SUB}>{fmtNum(live.fleet.usersWeek)} 本周登录</div>
	</div>
	<div class={TILE}>
		<span class="caps text-mist-400">击杀事件</span>
		<div class={BIG}>{fmtRate(killsPerSec, '')}<small class={SMALL}> 次击杀/秒</small></div>
		<div class={SUB}>
			{fmtRate(postsPerSec, '')} 次发送/秒，来自 {fmtNum(live.fleet.serversFeeding)} 台服务器
		</div>
	</div>
	<div class={TILE}>
		<span class="caps text-mist-400">观测记录</span>
		<div class={BIG}>{fmtRate(obsPerSec, '')}<small class={SMALL}> /s</small></div>
		<div class={SUB}>
			平均 {ms(obsMean)} · {worker?.behind ?? '…'} 滞后 · {worker?.stuck ?? '…'} 卡住
		</div>
	</div>
</div>

<div class="mb-3 grid gap-3 lg:grid-cols-3">
	<div class="panel px-5 py-4">
		<div class="mb-1 flex items-center gap-2">
			<span class="caps text-mist-400">后台进程</span>
			{#if worker?.owner}<Badge tone="ok">持有租约</Badge>{/if}
		</div>
		{#if worker}
			<div class="kv">
				<span class="text-mist-400">版本</span><span class={buildsDiffer ? 'text-warn' : ''}
					>{build(worker.process.build)}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">等级</span><span class="text-right"
					>{worker.tiers.watched} 监控中 · {worker.tiers.hot} 繁忙 · {worker.tiers.idle} 空闲 · {worker
						.tiers.offline} 无法连接</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">发送中</span><span class="text-right"
					>{worker.active} / {worker.concurrency} · 通道繁忙 {worker.lanes.busy}，排队中 {worker
						.lanes.queued}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">心跳</span><span
					>{worker.beatAgoMs === null ? '—' : `${secs(worker.beatAgoMs)} 前`}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">发送记录</span><span class="text-right"
					>{worker.delivery.pending} 等待中{#if worker.delivery.oldestMs !== null}
						（最早 {secs(worker.delivery.oldestMs)}){/if} · {fmtRate(deliveriesPerMin, ' / min')} · {fmtNum(
						worker.delivery.failed
					)} 启动以来失败</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">内存</span><span
					>{fmtBytes(worker.process.rssBytes)} 内存占用{lag(worker.process.eventLoopLagP99)}</span
				>
			</div>
		{:else}
			<p class="note">后台进程未响应健康检查。</p>
		{/if}
	</div>

	<div class="panel px-5 py-4">
		<div class="mb-1 flex items-center gap-2">
			<span class="caps text-mist-400">击杀事件</span><Badge>网页进程</Badge>
		</div>
		<div class="kv">
			<span class="text-mist-400">上报数据的服务器</span><span
				>{fmtNum(live.fleet.serversFeeding)} of {fmtNum(live.fleet.serversOk)} 在线</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">发送条数</span><span
				>{fmtRate(postsPerSec)} · {fmtRate(killsPerSec, '')} 次击杀/秒</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">已跳过 · 重复项</span><span
				>{fmtRate(skippedPerSec)} · {fmtRate(dupesPerSec)}</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">已拒绝（令牌错误）</span><span
				class={web.feed.unauthorized ? 'text-warn' : ''}
				>{fmtNum(web.feed.unauthorized)} 自启动以来</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">已拒绝 · 触发限流</span><span
				>{fmtNum(web.feed.rejected)} · {fmtNum(web.rateLimited.feed)} 自启动以来</span
			>
		</div>
	</div>

	<div class="panel px-5 py-4">
		<div class="mb-1 flex items-center gap-2">
			<span class="caps text-mist-400">网页</span><Badge>网页进程</Badge>
		</div>
		<div class="kv">
			<span class="text-mist-400">版本</span><span class={buildsDiffer ? 'text-warn' : ''}
				>{build(web.build)}</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">请求数</span><span>{fmtRate(reqPerSec)} · 平均 {ms(reqMean)}</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">公开页面</span><span
				>{fmtRate(publicPerSec)} · {fmtNum(live.fleet.serversPublic)} 台服务器公开</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">服务器错误（5xx）</span><span
				class={web.requests.errors ? 'text-danger' : ''}
				>{fmtNum(web.requests.errors)} 自启动以来</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">触发限流</span><span
				>{fmtNum(web.rateLimited.total)} 自启动以来</span
			>
		</div>
		<div class="kv">
			<span class="text-mist-400">内存</span><span
				>{fmtBytes(web.rssBytes)} 内存占用{lag(web.eventLoopLagP99)}</span
			>
		</div>
	</div>
</div>

<div class="mb-3 grid gap-3 lg:grid-cols-[2fr_1fr]">
	<div class="panel px-5 py-4">
		<div class="mb-3 flex items-center gap-2">
			<span class="caps text-mist-400">数据库</span><Badge class="whitespace-nowrap"
				>{fmtBytes(live.database.bytes)}</Badge
			>
			<span class="ml-auto text-[12px] text-mist-600">目录中的大小；行数为估算值</span>
		</div>
		<div class="table-wrap">
			<table>
				<thead>
					<tr
						><th>表格</th><th class="num">行数</th><th class="num">大小</th><th class="w-[36%]"
							>分享</th
						></tr
					>
				</thead>
				<tbody>
					{#each shown as t (t.name)}
						<tr>
							<td>{t.name}</td>
							<td class="num whitespace-nowrap">{fmtCompact(t.rows)}</td>
							<td class="num whitespace-nowrap">{fmtBytes(t.bytes)}</td>
							<td
								><div class="progress min-w-[120px]">
									<span style="width:{share(t.bytes)}%"></span>
								</div></td
							>
						</tr>
					{/each}
					{#if rest > 0}
						<tr>
							<td class="text-mist-400">其他</td>
							<td class="num text-mist-400">—</td>
							<td class="num whitespace-nowrap text-mist-400">{fmtBytes(rest)}</td>
							<td>
								<div class="progress min-w-[120px]">
									<span class="bg-mist-600" style="width:{share(rest)}%"></span>
								</div>
							</td>
						</tr>
					{/if}
				</tbody>
			</table>
		</div>
		<p class="note">
			每台服务器每天约增加 4,300 行采样数据，并每小时汇总；数据不会删除。使用 TimescaleDB
			时，旧采样和击杀记录会压缩。
		</p>
	</div>

	<div class="flex flex-col gap-3">
		<div class="panel px-5 py-4">
			<div class="mb-1 flex items-center gap-2">
				<span class="caps text-mist-400">出现过的玩家</span>
				<span class="ml-auto text-[12px] text-mist-600"
					>{live.seen ? `counted ${fmtAgo(live.seen.at)}` : 'counting…'}</span
				>
				<button type="button" class="btn btn-sm" disabled={recounting} onclick={recount}
					>重新统计</button
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">今天</span><span
					>{live.seen ? fmtNum(live.seen.today) : '…'}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">最近 30 天</span><span
					>{live.seen ? fmtNum(live.seen.month) : '…'}</span
				>
			</div>
			<div class="kv">
				<span class="text-mist-400">全部时间</span><span
					>{live.seen ? fmtNum(live.seen.all) : '…'}</span
				>
			</div>
		</div>
		<div class="panel px-5 py-4">
			<span class="mb-1 block caps text-mist-400">按版本统计服务器</span>
			{#each live.builds as b (b.build)}
				<div class="kv">
					<span class={b.build ? '' : 'text-mist-400'}>{b.build || '尚未读取'}</span><span
						>{fmtNum(b.count)}</span
					>
				</div>
			{:else}
				<p class="note">暂无服务器。</p>
			{/each}
		</div>
	</div>
</div>

<div class="flex flex-wrap items-center gap-4 panel px-5 py-3.5">
	<span class="caps text-mist-400">Prometheus</span>
	<span class="text-[13px]">
		这里的所有指标和历史数据均通过 <code class="chip">/metrics</code> 在 Web
		和工作进程上导出，使用令牌 <code class="chip">METRICS_TOKEN</code>；可供现有的 Prometheus 和
		Grafana 使用。README 提供抓取配置和可导入的仪表板。
	</span>
	{#if live.metricsOn}<Badge tone="ok">开启</Badge>{:else}<Badge tone="warn">关闭</Badge>{/if}
</div>
