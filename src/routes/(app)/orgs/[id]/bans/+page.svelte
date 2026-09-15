<script lang="ts">
	import BanListPage from '../BanListPage.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<BanListPage entries={data.entries} lists={data.lists} org={data.org} />
