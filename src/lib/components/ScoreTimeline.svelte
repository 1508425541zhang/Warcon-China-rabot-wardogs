<script lang="ts">
	// The score of each faction over a match, from the samples of its window: one line per
	// faction in its colour, from 0 to the cap, over the match's length. Inline SVG that scales
	// with its box; the legend is the coloured names above it.
	import { DEFAULT_SCORE_CAP } from '$lib/match';
	import { factionColor } from '$lib/format';
	import { fmtLength, type TimelinePoint } from '$lib/matches';

	let {
		points,
		factions,
		durationSeconds
	}: {
		points: TimelinePoint[];
		factions: { name: string; colorHex: string | null }[];
		durationSeconds: number;
	} = $props();

	const W = 640;
	const H = 180;
	const PAD = { top: 10, right: 12, bottom: 26, left: 34 };
	let cap = $derived(Math.max(DEFAULT_SCORE_CAP, ...points.flatMap((p) => p.slice(1))));
	let span = $derived(Math.max(durationSeconds, points[points.length - 1]?.[0] ?? 0, 60));
	const x = (sec: number) => PAD.left + (sec / span) * (W - PAD.left - PAD.right);
	const y = (score: number) => PAD.top + (1 - score / cap) * (H - PAD.top - PAD.bottom);
	let lines = $derived(
		factions.map((f, i) => ({
			name: f.name,
			color: f.colorHex || factionColor(f.name),
			d: points
				.map((p, k) => `${k ? 'L' : 'M'}${x(p[0]).toFixed(1)},${y(p[i + 1] ?? 0).toFixed(1)}`)
				.join(' ')
		}))
	);
	let ticks = $derived([0, 0.25, 0.5, 0.75, 1].map((r) => Math.round(cap * r)));
	let stops = $derived([0, 0.5, 1].map((r) => Math.round(span * r)));
</script>

<div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-[12.5px]">
	{#each lines as l (l.name)}
		<span class="inline-flex items-center gap-1.5">
			<span class="inline-block h-2.5 w-2.5 rounded-full" style="background:{l.color}"
			></span>{l.name}
		</span>
	{/each}
</div>
<svg viewBox="0 0 {W} {H}" class="mt-1 h-auto w-full" role="img" aria-label="Score over the match">
	{#each ticks as t (t)}
		<line x1={PAD.left} x2={W - PAD.right} y1={y(t)} y2={y(t)} stroke="#2f2f35" stroke-width="1" />
		<text x={PAD.left - 6} y={y(t) + 4} text-anchor="end" font-size="11" fill="#8a8a90">{t}</text>
	{/each}
	{#each stops as s (s)}
		<text
			x={x(s)}
			y={H - 8}
			text-anchor={s === 0 ? 'start' : s === span ? 'end' : 'middle'}
			font-size="11"
			fill="#8a8a90">{fmtLength(s)}</text
		>
	{/each}
	{#each lines as l (l.name)}
		{#if l.d}<path
				d={l.d}
				fill="none"
				stroke={l.color}
				stroke-width="2"
				stroke-linejoin="round"
			/>{/if}
	{/each}
	{#if !points.length}
		<text x={W / 2} y={H / 2} text-anchor="middle" font-size="12" fill="#55555c"
			>No samples of this match were kept.</text
		>
	{/if}
</svg>
