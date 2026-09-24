<script lang="ts">
	import '../app.css';
	import Toasts from '$lib/components/Toasts.svelte';
	import ConfirmHost from '$lib/components/ConfirmHost.svelte';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();
	// The fallback title, for a page that sets none. Held in a $derived so a data reload, which
	// hands this layout a new `data` with the same name in it, does not write it again over the
	// title of the page one is on.
	let appName = $derived(data.appName);
</script>

<svelte:head>
	<title>{appName}</title>
</svelte:head>

{@render children()}
<Toasts />
<ConfirmHost />
