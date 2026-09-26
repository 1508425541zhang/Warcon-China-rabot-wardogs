<script lang="ts">
	import { causeLabel } from '$lib/causes';
	import { fmtTime } from '$lib/format';
	import type { WeaponDistanceRow } from '$lib/server/weapon-distance';
	import type { weaponDistanceDistribution } from '$lib/weapon-distance-distribution';
	type ChartRow = WeaponDistanceRow & {
		distribution: ReturnType<typeof weaponDistanceDistribution>;
	};
	let {
		data
	}: {
		data: { days: number; minimumSamples: number; refreshedAt: string; rows: ChartRow[] };
	} = $props();
	const x = (value: number, row: ChartRow) => 48 + (420 * value) / row.distribution.upper;
	const y = (count: number, row: ChartRow) => 190 - (140 * count) / row.distribution.peak;
	const points = (row: ChartRow) =>
		row.distribution.bins.map((bin) => `${x(bin.center, row)},${y(bin.count, row)}`).join(' ');
</script>

<section class="mb-4 panel p-4">
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h3 class="text-base font-semibold text-white">武器击杀距离 · 全服分布曲线</h3>
		<span class="rounded border border-white/10 px-2 py-1 text-xs text-mist-300"
			>仅供管理员参考 · 不参与反作弊判断</span
		>
	</div>
	<p class="mt-2 text-xs text-mist-400">
		过去 {data.days} 天，同武器按玩家平均击杀距离由远到近排名，同值并列。每位玩家有效击杀至少 {data.minimumSamples}
		次才参与排名；少于 {data.minimumSamples} 次显示 No（数据不足）。曲线统计本服同武器达标玩家，每人以自己的平均击杀距离贡献一个样本。
	</p>
	<p class="mt-1 text-xs text-mist-400">
		仅使用已回传的武器距离，排除异常距离、自杀和已标记的友军击杀。不同地图与玩法会影响距离，排名不代表作弊概率。数据每分钟更新。
	</p>
	{#if data.rows.length}
		<div class="mt-4 grid gap-4 lg:grid-cols-2">
			{#each data.rows as row (row.cause)}
				<div class="rounded-ctl border border-white/10 p-3">
					<div class="flex items-center justify-between gap-3">
						<h4 class="font-semibold text-white" title={row.cause}>{causeLabel(row.cause)}</h4>
						<span class="font-mono text-sm text-accent"
							>{row.rank === null
								? 'No（数据不足）'
								: `第 ${row.rank} / ${row.eligiblePlayers} 名`}</span
						>
					</div>
					<div class="mt-3">
						<svg
							viewBox="0 0 500 240"
							class="w-full"
							role="img"
							aria-label={`${causeLabel(row.cause)}全服玩家平均击杀距离分布，${row.distribution.players} 位达标玩家，本人 ${row.average.toFixed(1)} 米，${row.rank === null ? 'No，数据不足，未参与排名' : `第 ${row.rank} 名`}`}
						>
							<title>{causeLabel(row.cause)} · 全服同武器玩家距离分布</title>
							<text x="48" y="20" fill="currentColor" font-size="11"
								>玩家人数（每段 {row.distribution.step.toFixed(1)} 米）</text
							>
							{#each [0, 0.5, 1] as tick}
								<line
									x1="48"
									x2="468"
									y1={y(tick * row.distribution.peak, row)}
									y2={y(tick * row.distribution.peak, row)}
									stroke="currentColor"
									opacity="0.12"
								/>
								<text
									x="40"
									y={y(tick * row.distribution.peak, row) + 4}
									text-anchor="end"
									fill="currentColor"
									font-size="11"
									>{(tick * row.distribution.peak).toFixed(
										tick === 0.5 && row.distribution.peak % 2 ? 1 : 0
									)}</text
								>
							{/each}
							{#if row.distribution.players > 0}
								<polygon points={`48,190 ${points(row)} 468,190`} fill="#38bdf8" opacity="0.10" />
								<polyline
									points={points(row)}
									fill="none"
									stroke="#38bdf8"
									stroke-width="2.5"
									stroke-linejoin="round"
								/>
								{#each row.distribution.bins as bin}
									<circle cx={x(bin.center, row)} cy={y(bin.count, row)} r="3" fill="#38bdf8"
										><title>{bin.from.toFixed(1)}–{bin.to.toFixed(1)} 米：{bin.count} 人</title
										></circle
									>
								{/each}
							{:else}<text x="250" y="105" text-anchor="middle" fill="currentColor" font-size="13"
									>No：暂无达标玩家，无法绘制分布</text
								>{/if}
							<line
								x1={x(row.average, row)}
								x2={x(row.average, row)}
								y1="42"
								y2="190"
								stroke="#fbbf24"
								stroke-width="2"
								stroke-dasharray="5 4"
							/>
							<circle cx={x(row.average, row)} cy="190" r="4" fill="#fbbf24" />
							<text
								x={Math.min(410, Math.max(108, x(row.average, row)))}
								y="36"
								text-anchor="middle"
								fill="#fbbf24"
								font-size="12">本人 {row.average.toFixed(1)} 米</text
							>
							{#each [0, 0.25, 0.5, 0.75, 1] as tick}
								<text
									x={48 + 420 * tick}
									y="209"
									text-anchor="middle"
									fill="currentColor"
									font-size="11">{(row.distribution.upper * tick).toFixed(0)}</text
								>
							{/each}
							<text x="258" y="231" text-anchor="middle" fill="currentColor" font-size="11"
								>玩家平均击杀距离（米） →</text
							>
						</svg>
						<p class="text-xs text-mist-400">
							<span style="color:#38bdf8">蓝线：全服玩家分布</span> ·
							<span style="color:#fbbf24">黄线：本人位置</span>{row.rank === null
								? '（本人样本不足，仅标位置，不进入曲线及排名）'
								: ''}
						</p>
						{#if row.distribution.players < 10}<p class="mt-1 text-xs text-mist-400">
								当前仅 {row.distribution.players} 位达标玩家，分布较稀疏；曲线为实际分段人数连线，不拟合正态分布。
							</p>{/if}
					</div>
					<p class="mt-3 text-xs text-mist-400">
						本人有效击杀 {row.samples} 次 · 最远 {row.maximum.toFixed(1)} 米 · 达标玩家 {row.eligiblePlayers}
						人
					</p>
				</div>
			{/each}
		</div>
	{:else}<p class="mt-4 rounded-ctl border border-white/10 p-4 text-sm text-mist-400">
			No（数据不足）：过去 {data.days} 天暂无该玩家的有效武器击杀距离记录。
		</p>{/if}
	<p class="mt-3 text-xs text-mist-400">
		统计更新：{fmtTime(data.refreshedAt)} · 每张图独立刻度，单位：米
	</p>
</section>
