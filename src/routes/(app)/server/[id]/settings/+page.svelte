<script lang="ts">
	// Settings that belong to the panel's record of this server rather than to the game: the
	// Discord channels carrying its card or team kills, and the public pages.
	import DiscordChannels from './DiscordChannels.svelte';
	import PublicPages from './PublicPages.svelte';
	import StatsPurge from './StatsPurge.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="space-y-4">
	<DiscordChannels {data} />
	<PublicPages {data} />
	{#if data.server.manager}<StatsPurge {data} />{/if}
</div>
