<script lang="ts">
	// A "⋯" button that drops a small menu of the rarer actions on a row, so a list of rows does
	// not carry three buttons each. Items are rendered by the caller; any click inside closes it,
	// and so does any click outside, including on another row's ⋯.
	import type { Snippet } from 'svelte';
	let {
		label = 'More actions',
		children
	}: {
		label?: string;
		children: Snippet;
	} = $props();
	let open = $state(false);
	let wrap: HTMLDivElement;
</script>

<svelte:window
	onclick={(e) => {
		if (!wrap.contains(e.target as Node)) open = false;
	}}
	onkeydown={(e) => e.key === 'Escape' && (open = false)}
/>

<div class="relative shrink-0" bind:this={wrap}>
	<button
		type="button"
		class="btn btn-sm w-8 px-0 text-[15px] leading-none tracking-[0.1em] {open ? 'bg-ink-700' : ''}"
		aria-label={label}
		aria-haspopup="menu"
		aria-expanded={open}
		onclick={() => (open = !open)}>⋯</button
	>
	{#if open}
		<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
		<div
			class="absolute top-[calc(100%+6px)] right-0 z-40 min-w-[180px] rise rounded-card border border-black bg-ink-900 p-1 shadow-pop"
			role="menu"
			tabindex="-1"
			onclick={() => (open = false)}
		>
			{@render children()}
		</div>
	{/if}
</div>
