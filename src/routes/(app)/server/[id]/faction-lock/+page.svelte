<script lang="ts">
	import FactionLockSettings from '$lib/components/FactionLockSettings.svelte';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
</script>

<svelte:head><title>禁止自行换边</title></svelte:head>
<FactionLockSettings {data} />
