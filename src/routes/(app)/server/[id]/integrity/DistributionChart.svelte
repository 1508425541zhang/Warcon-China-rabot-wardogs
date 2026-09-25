<script lang="ts">
	import type { MetricAssessment } from '$lib/server/integrity/statistics';
	let { metric, lang }: { metric: MetricAssessment; lang: 'zh' | 'en' } = $props();
	const labels: Record<string, [string, string]> = {
		kpm180: ['180 秒步兵 KPM', '180s infantry KPM'],
		uniqueVictims: ['独立受害者', 'Unique victims'],
		maxKills15s: ['15 秒最多击杀', 'Max kills in 15s'],
		medianKillInterval: ['击杀间隔中位数', 'Median kill interval'],
		headshotRate: ['爆头率', 'Headshot rate'],
		penetrationRate: ['穿透率', 'Penetration rate']
	};
	const fmt = (n: number) =>
		metric.code === 'headshotRate' || metric.code === 'penetrationRate'
			? `${(n * 100).toFixed(1)}%`
			: n.toFixed(2);
	const quality = (count: number) =>
		count < 1000
			? lang === 'zh'
				? '低样本'
				: 'Low sample'
			: count < 5000
				? lang === 'zh'
					? '常规样本'
					: 'Normal sample'
				: lang === 'zh'
					? '高样本'
					: 'High sample';
	let histogram = $derived(metric.histogram);
	let minimum = $derived(histogram[0]?.from ?? 0);
	let maximum = $derived(histogram.at(-1)?.to ?? 1);
	let span = $derived(Math.max(maximum - minimum, 0.001));
	let tallest = $derived(Math.max(1, ...histogram.map((bin) => bin.count)));
	let actual = $derived(
		histogram
			.map((bin, i) => {
				const x = 12 + ((i + 0.5) * 336) / Math.max(1, histogram.length);
				const y = 104 - (bin.count * 84) / tallest;
				return `${i ? 'L' : 'M'} ${x.toFixed(1)} ${y.toFixed(1)}`;
			})
			.join(' ')
	);
	let marker = $derived(Math.max(12, Math.min(348, 12 + ((metric.value - minimum) * 336) / span)));
	let spread = $derived(Math.max(metric.mad ?? 0, (metric.p95 - metric.median) / 1.645, span / 30));
	let reference = $derived(
		Array.from({ length: 61 }, (_, i) => {
			const x = 12 + (i * 336) / 60;
			const value = minimum + (i * span) / 60;
			const y = 104 - 80 * Math.exp(-0.5 * ((value - metric.median) / spread) ** 2);
			return `${i ? 'L' : 'M'} ${x.toFixed(1)} ${y.toFixed(1)}`;
		}).join(' ')
	);
</script>

<article class="rounded-ctl border border-white/10 bg-black/10 p-4">
	<div class="flex flex-wrap items-baseline justify-between gap-2">
		<h4 class="font-semibold text-white">
			{labels[metric.code]?.[lang === 'zh' ? 0 : 1] ?? metric.code}
		</h4>
		<span class="font-mono text-sm text-accent"
			>P{(metric.extremenessPercentile * 100).toFixed(2)}</span
		>
	</div>
	<svg
		class="mt-3 w-full"
		viewBox="0 0 360 132"
		role="img"
		aria-label={lang === 'zh'
			? '历史分布实线、参考钟形虚线及当前玩家位置'
			: 'Actual distribution, reference bell and player position'}
	>
		<line x1="12" y1="104" x2="348" y2="104" stroke="currentColor" opacity="0.3" />
		<path
			d={reference}
			fill="none"
			stroke="#a9b4c5"
			stroke-width="1.5"
			stroke-dasharray="5 4"
			opacity="0.65"
		/>
		<path d={actual} fill="none" stroke="#69d6e3" stroke-width="2.5" />
		<line x1={marker} y1="12" x2={marker} y2="104" stroke="#f8b95f" stroke-width="2" />
		<circle cx={marker} cy="12" r="4" fill="#f8b95f" />
		<text
			x={Math.max(48, Math.min(312, marker))}
			y="126"
			text-anchor="middle"
			fill="#f8b95f"
			font-size="11">▲ {lang === 'zh' ? '当前玩家' : 'Player'}</text
		>
	</svg>
	<p class="text-xs text-mist-400">
		{lang === 'zh'
			? '实线：真实历史频率；虚线：仅供视觉参考的钟形曲线，不参与判断。'
			: 'Solid: actual history. Dashed: visual bell reference only; never used for decisions.'}
	</p>
	<div class="mt-3 grid grid-cols-2 gap-x-3 gap-y-1 text-xs sm:grid-cols-4">
		<div>
			{lang === 'zh' ? '当前' : 'Current'} <strong class="text-white">{fmt(metric.value)}</strong>
		</div>
		<div>
			{lang === 'zh' ? '中位数' : 'Median'} <strong class="text-white">{fmt(metric.median)}</strong>
		</div>
		<div>P95 <strong class="text-white">{fmt(metric.p95)}</strong></div>
		<div>P99 <strong class="text-white">{fmt(metric.p99)}</strong></div>
		<div>P99.9 <strong class="text-white">{fmt(metric.p999)}</strong></div>
		<div>
			{lang === 'zh' ? '样本' : 'Samples'}
			<strong class="text-white">{metric.sampleCount.toLocaleString()}</strong>
			· {quality(metric.sampleCount)}
		</div>
		<div>
			{lang === 'zh' ? '周期' : 'Period'}
			<strong class="text-white">{metric.windowDays} {lang === 'zh' ? '天' : 'days'}</strong>
		</div>
		<div>
			{lang === 'zh' ? '方向' : 'Tail'}
			<strong class="text-white"
				>{metric.tail === 'lower'
					? lang === 'zh'
						? '越低越异常'
						: 'Lower'
					: lang === 'zh'
						? '越高越异常'
						: 'Upper'}</strong
			>
		</div>
	</div>
	<p class="mt-2 text-xs text-mist-400">
		{metric.populationBucket ?? (lang === 'zh' ? '全人口' : 'All populations')} · {lang === 'zh' &&
		metric.weaponCategory === 'INFANTRY'
			? '步兵武器'
			: metric.weaponCategory}
		· {metric.map ?? (lang === 'zh' ? '全地图' : 'All maps')}
	</p>
</article>
