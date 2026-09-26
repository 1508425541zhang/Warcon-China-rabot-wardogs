<script lang="ts">
	import type { PlaytimeDistribution } from '$lib/server/playtime-distribution';
	let { data }: { data: PlaytimeDistribution } = $props();
	let cumulative = $state(false);
	let maximum = $derived(cumulative ? 100 : Math.max(1, ...data.bins.map((b) => b.count)));
	const x = (i: number) => 60 + (i * 630) / Math.max(1, data.bins.length - 1);
	let points = $derived(
		data.bins
			.map((b, i) => `${x(i)},${230 - ((cumulative ? b.cumulative : b.count) / maximum) * 190}`)
			.join(' ')
	);
</script>

<section class="mb-4 panel">
	<div class="mb-3 flex flex-wrap items-center gap-3">
		<h3 class="label-sm mb-0">WARDOGS 生涯游戏时长分布</h3>
		<button class="ml-auto btn btn-sm" onclick={() => (cumulative = !cumulative)}
			>{cumulative ? '切换人数分布' : '切换累计占比'}</button
		>
	</div>
	<p class="mb-3 text-xs text-mist-400">
		统计所选时间范围内在本服出现的玩家，按 SteamID 去重。时长为 Steam
		返回的累计游戏时间（含菜单、挂机等），并非本局时长或纯战斗时长。后台每 24
		小时刷新；仅供管理员参考。
	</p>
	<p class="mb-3">
		总人数 {data.total} · 已知 {data.available} · 未知 {data.unknown}（待查询／刷新 {data.pending}，查询失败
		{data.errors}）
	</p>
	{#if !data.enabled}<p class="mb-3">
			未配置 Steam Web API Key，请在服务器环境变量中设置 STEAM_API_KEY。
		</p>{/if}
	{#if data.p80Minutes !== null}
		<p class="mb-3 font-semibold">
			公开样本中至少 80% 的玩家，累计时长不超过 {Math.floor(data.p80Minutes / 60)} 小时 {data.p80Minutes %
				60} 分钟。
		</p>
		<p class="text-xs text-mist-400">
			此比例仅针对已知时长玩家，不代表所有玩家，也不是作弊概率。分位数按原始分钟数计算，同值合并。
		</p>
		<svg
			viewBox="0 0 740 285"
			class="w-full"
			role="img"
			aria-label={cumulative ? '按时长区间上界累计的已知玩家比例折线' : '各时长区间的玩家人数折线'}
		>
			<line x1="60" y1="230" x2="710" y2="230" stroke="currentColor" opacity=".3" />
			{#each [0, 0.5, 1] as ratio}
				<line
					x1="60"
					y1={230 - ratio * 190}
					x2="710"
					y2={230 - ratio * 190}
					stroke="currentColor"
					opacity=".1"
				/>
				<text x="50" y={234 - ratio * 190} text-anchor="end" fill="currentColor" font-size="12"
					>{(ratio * maximum).toFixed(0)}{cumulative ? '%' : ''}</text
				>
			{/each}
			{#if cumulative}<line
					x1="60"
					y1="78"
					x2="710"
					y2="78"
					stroke="#fbbf24"
					stroke-dasharray="5 5"
				/><text x="710" y="72" text-anchor="end" fill="#fbbf24" font-size="12">80%</text>{/if}
			<polyline {points} fill="none" stroke="#38bdf8" stroke-width="2" />
			{#each data.bins as b, i}
				<circle
					cx={x(i)}
					cy={230 - ((cumulative ? b.cumulative : b.count) / maximum) * 190}
					r="3"
					fill="#38bdf8"
					><title>{b.from} 至不足 {b.to} 小时：{b.count} 人；累计 {b.cumulative.toFixed(1)}%</title
					></circle
				>
				{#if i % Math.max(1, Math.ceil(data.bins.length / 8)) === 0 || i === data.bins.length - 1}<text
						x={x(i)}
						y="250"
						text-anchor="middle"
						fill="currentColor"
						font-size="11">{b.to}</text
					>{/if}
			{/each}
			<text x="380" y="278" text-anchor="middle" fill="currentColor" font-size="12"
				>时长区间上界（不足该小时数）；折线连接分组统计点</text
			>
		</svg>
	{:else}<p class="py-6 text-mist-400">
			暂无公开时长样本。后台查询完成后自动显示，未公开时长不会记为 0。
		</p>{/if}
	<div class="table-wrap">
		<table>
			<thead><tr><th>累计时长区间</th><th>人数</th><th>占已知样本</th><th>累计占比</th></tr></thead
			><tbody>
				<tr><td>未知／未公开／未返回</td><td>{data.unknown}</td><td>不参与</td><td>—</td></tr>
				{#each data.bins as b}<tr
						><td>{b.from} 至不足 {b.to} 小时</td><td>{b.count}</td><td>{b.percent.toFixed(1)}%</td
						><td>{b.cumulative.toFixed(1)}%</td></tr
					>{/each}
			</tbody>
		</table>
	</div>
</section>
