<script lang="ts">
	import { causeLabel } from '$lib/causes';
	import { fmtTime } from '$lib/format';
	import type { WeaponDistanceRow } from '$lib/server/weapon-distance';
	let {
		data
	}: {
		data: { days: number; minimumSamples: number; refreshedAt: string; rows: WeaponDistanceRow[] };
	} = $props();
	const width = (value: number, row: WeaponDistanceRow) =>
		(100 * value) / Math.max(row.average, row.serverAverage, 1);
</script>

<section class="mb-4 panel p-4">
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h3 class="text-base font-semibold text-white">武器击杀距离 · 全服排名</h3>
		<span class="rounded border border-white/10 px-2 py-1 text-xs text-mist-300"
			>仅供管理员参考 · 不参与反作弊判断</span
		>
	</div>
	<p class="mt-2 text-xs text-mist-400">
		过去 {data.days} 天，同武器按玩家平均击杀距离由远到近排名，同值并列。每位玩家有效击杀至少 {data.minimumSamples}
		次才参与排名；少于 {data.minimumSamples} 次显示 No（数据不足）。全服均值按有效击杀次数加权，包含样本不足的玩家。
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
					<div
						class="mt-3 space-y-3"
						role="img"
						aria-label={`${causeLabel(row.cause)}：本人平均 ${row.average.toFixed(1)} 米，全服平均 ${row.serverAverage.toFixed(1)} 米；${row.rank === null ? 'No，数据不足' : `第 ${row.rank} 名，共 ${row.eligiblePlayers} 人`}`}
					>
						<div>
							<div class="mb-1 flex justify-between text-xs text-mist-300">
								<span>本人均值{row.rank === null ? '（仅展示）' : ''}</span><span
									>{row.average.toFixed(1)} 米</span
								>
							</div>
							<div class="h-3 rounded bg-white/5">
								<div
									class="h-3 rounded bg-accent"
									style:width={`${width(row.average, row)}%`}
								></div>
							</div>
						</div>
						<div>
							<div class="mb-1 flex justify-between text-xs text-mist-300">
								<span>全服均值</span><span>{row.serverAverage.toFixed(1)} 米</span>
							</div>
							<div class="h-3 rounded bg-white/5">
								<div
									class="h-3 rounded bg-mist-400"
									style:width={`${width(row.serverAverage, row)}%`}
								></div>
							</div>
						</div>
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
