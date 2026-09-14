<script lang="ts">
	import type { Snippet } from 'svelte';
	let {
		title = '',
		wide = false,
		onclose,
		children,
		actions
	}: {
		title?: string;
		wide?: boolean;
		onclose: () => void;
		children: Snippet;
		actions?: Snippet;
	} = $props();

	let box: HTMLDivElement | undefined = $state();

	const FOCUSABLE =
		'a[href], button:not([disabled]), input:not([type=hidden]):not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

	// Focus the first field on open and hand focus back to whatever opened the dialog on close.
	$effect(() => {
		const opener = document.activeElement as HTMLElement | null;
		const first = box?.querySelector<HTMLElement>(
			'input:not([type=hidden]):not([disabled]), textarea, select, button:not([data-close])'
		);
		first?.focus();
		return () => {
			if (opener?.isConnected) opener.focus();
		};
	});

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			onclose();
			return;
		}
		// Keep Tab inside the dialog: the page behind it is not interactive while it is open.
		if (e.key === 'Tab' && box) {
			const items = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)];
			if (!items.length) return;
			const first = items[0];
			const last = items[items.length - 1];
			const active = document.activeElement;
			if (e.shiftKey && (active === first || !box.contains(active))) {
				e.preventDefault();
				last.focus();
			} else if (!e.shiftKey && (active === last || !box.contains(active))) {
				e.preventDefault();
				first.focus();
			}
		}
	}
</script>

<svelte:window {onkeydown} />

<div class="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4">
	<button
		type="button"
		class="absolute inset-0 cursor-default bg-black/70"
		aria-label="Close dialog"
		data-close
		tabindex="-1"
		onclick={onclose}
	></button>
	<div
		bind:this={box}
		class="relative max-h-[calc(100dvh-1.5rem)] w-full rise overflow-y-auto panel shadow-pop {wide
			? 'max-w-3xl'
			: 'max-w-lg'}"
		role="dialog"
		aria-modal="true"
		aria-label={title || 'Dialog'}
	>
		{#if title}<h3 class="mb-4 text-[15px] font-semibold">{title}</h3>{/if}
		{@render children()}
		{#if actions}<div class="mt-5 flex flex-wrap justify-end gap-2">{@render actions()}</div>{/if}
	</div>
</div>
