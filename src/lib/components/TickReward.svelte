<script lang="ts">
	// What one score tick pays each zone at the given cadence, beside a tick slider.
	import { tickReward } from '$lib/tick';
	import { fmtCash } from '$lib/cash';

	let { seconds, class: cls = '' }: { seconds: number | null | undefined; class?: string } =
		$props();
	let r = $derived(tickReward(seconds));
</script>

{#if r}
	<div class="flex flex-wrap items-baseline gap-x-4 gap-y-1 text-[12.5px] {cls}">
		<span class="inline-flex items-baseline gap-1.5"
			><span class="caps text-mist-400">控制区</span><span class="font-mono tabular"
				>{fmtCash(r.control)}</span
			></span
		>
		<span class="inline-flex items-baseline gap-1.5"
			><span class="caps text-mist-400">热点区域</span><span class="font-mono tabular"
				>{fmtCash(r.hot)}</span
			></span
		>
		<span class="text-mist-600">每次更新 · ×{r.band.multiplier.toFixed(1)} at {r.band.label}</span>
	</div>
{/if}
