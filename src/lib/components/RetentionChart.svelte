<script lang="ts">
	import type { MatchRetention } from '$lib/server/match-retention';
	let { points }: { points: MatchRetention[] } = $props();
	let table = $state(false);
	let max = $derived(Math.max(2, Math.ceil(Math.max(0, ...points.map((p) => p.total)) / 2) * 2));
	let width = $derived(Math.max(720, points.length * 76 + 70));
	const x = (i: number) => 65 + i * 76;
	const label = (p: MatchRetention) =>
		`${p.fromMap ?? '未知地图'} #${p.fromId} → ${p.toMap ?? '未知地图'} #${p.toId}`;
	const time = (p: MatchRetention) => new Date(p.startedAt).toLocaleString('zh-CN');
</script>

<section class="mb-4 panel">
	<div class="mb-2 flex flex-wrap items-center gap-3">
		<h3 class="label-sm mb-0">相邻对局玩家留存</h3>
		<button class="ml-auto btn btn-sm" onclick={() => (table = !table)}
			>{table ? '显示图表' : '查看数据表'}</button
		>
	</div>
	<p class="mb-3 text-xs text-mist-400">
		按 SteamID
		去重：上一局出现的玩家，在下一局再次出现即计留存（含离开后返回）；新玩家不计入。柱高＝上一局人数，留存率＝留存人数÷上一局人数。按下一局开始时间筛选，最多显示最近
		100 次换局。
	</p>
	<p class="mb-3 text-xs text-mist-400">
		进行中对局为暂定；缺少任一局名单显示“数据不足”。统计基于面板实际观测，停机或漏采可能影响结果，不代表全程连续在线。
	</p>
	{#if !points.length}
		<p class="py-8 text-center text-mist-400">暂无相邻对局记录，至少需要两局。</p>
	{:else if table}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>换局</th><th>下一局开始</th><th>上一局人数</th><th>留存</th><th>流失</th><th
							>留存率</th
						><th>状态</th></tr
					></thead
				>
				<tbody
					>{#each points as p (p.toId)}<tr
							><td>{label(p)}</td><td>{time(p)}</td><td>{p.total}</td><td>{p.retained ?? '—'}</td
							><td>{p.lost ?? '—'}</td><td>{p.percent === null ? '—' : `${p.percent}%`}</td><td
								>{p.retained === null ? '数据不足' : p.provisional ? '暂定' : '已结束'}</td
							></tr
						>{/each}</tbody
				>
			</table>
		</div>
	{:else}
		<div class="mb-2 flex gap-4 text-xs">
			<span style="color:#34d399">■ 留存人数</span><span style="color:#fbbf24">■ 流失人数</span
			><span class="text-mist-400">横轴：相邻对局编号；可左右滚动</span>
		</div>
		<!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard access to horizontal scrolling.) -->
		<div class="overflow-x-auto" tabindex="0" role="region" aria-label="玩家留存图表，可横向滚动">
			<svg
				{width}
				height="480"
				role="img"
				aria-label="上方留存流失人数堆叠柱状图，下方每局留存百分比折线图"
			>
				<text x="8" y="16" fill="#a3b3c2" font-size="12">人数</text>
				{#each [0, 0.5, 1] as t}<line
						x1="42"
						x2={width - 10}
						y1={200 - t * 160}
						y2={200 - t * 160}
						stroke="#334155"
					/><text x="38" y={204 - t * 160} text-anchor="end" fill="#a3b3c2" font-size="11"
						>{Math.ceil(max * t)}</text
					>{/each}
				<text x="8" y="266" fill="#a3b3c2" font-size="12">留存率</text>
				{#each [0, 50, 100] as t}<line
						x1="42"
						x2={width - 10}
						y1={430 - t * 1.4}
						y2={430 - t * 1.4}
						stroke="#334155"
					/><text x="38" y={434 - t * 1.4} text-anchor="end" fill="#a3b3c2" font-size="11"
						>{t}%</text
					>{/each}
				{#each points as p, i (p.toId)}
					<g
						><title
							>{label(p)} · {time(p)} · {p.retained === null
								? '数据不足'
								: `留存 ${p.retained} 人，流失 ${p.lost} 人，留存率 ${p.percent}%`}{p.provisional
								? '（暂定）'
								: ''}</title
						>
						{#if p.retained !== null && p.lost !== null && p.percent !== null}
							<rect
								x={x(i) - 20}
								y={200 - (p.retained / max) * 160}
								width="40"
								height={(p.retained / max) * 160}
								fill="#34d399"
								opacity={p.provisional ? 0.6 : 1}
							/>
							<rect
								x={x(i) - 20}
								y={200 - (p.total / max) * 160}
								width="40"
								height={(p.lost / max) * 160}
								fill="#fbbf24"
								opacity={p.provisional ? 0.6 : 1}
							/>
							<text
								x={x(i)}
								y={190 - (p.total / max) * 160}
								text-anchor="middle"
								fill="#e2e8f0"
								font-size="11">{p.total}</text
							>
							{#if i > 0 && points[i - 1].percent !== null}<line
									x1={x(i - 1)}
									y1={430 - points[i - 1].percent! * 1.4}
									x2={x(i)}
									y2={430 - p.percent * 1.4}
									stroke="#60a5fa"
									stroke-width="2"
									stroke-dasharray={p.provisional ? '5 4' : undefined}
								/>{/if}
							<circle cx={x(i)} cy={430 - p.percent * 1.4} r="4" fill="#60a5fa" />
							<text
								x={x(i)}
								y={420 - p.percent * 1.4}
								text-anchor="middle"
								fill="#93c5fd"
								font-size="11">{p.percent}%</text
							>
						{:else}<text x={x(i)} y="160" text-anchor="middle" fill="#a3b3c2" font-size="11"
								>数据不足</text
							>{/if}
						<text x={x(i)} y="219" text-anchor="middle" fill="#a3b3c2" font-size="10"
							>{p.fromId}→{p.toId}</text
						>
						<text x={x(i)} y="236" text-anchor="middle" fill="#a3b3c2" font-size="10"
							>{p.provisional ? '暂定' : '已结束'}</text
						>
						<text x={x(i)} y="453" text-anchor="middle" fill="#a3b3c2" font-size="10"
							>{p.fromId}→{p.toId}</text
						>
					</g>
				{/each}
			</svg>
		</div>
	{/if}
</section>
