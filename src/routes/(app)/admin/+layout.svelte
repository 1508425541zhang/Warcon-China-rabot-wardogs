<script lang="ts">
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();

	const TABS = [
		['/admin', 'Overview'],
		['/admin/users', 'Users'],
		['/admin/settings', 'Settings']
	] as const;
	const isCurrent = (path: string) =>
		path === '/admin' ? page.url.pathname === '/admin' : page.url.pathname.startsWith(path);
</script>

<h1 class="mb-3 text-xl font-semibold tracking-tight">Admin</h1>
<nav class="strip mb-5 gap-1 border-b border-white/8 pb-3" aria-label="Admin sections">
	{#each TABS as [path, label] (path)}
		<a href={path} class="tab-link {isCurrent(path) ? 'tab-link-active' : ''}">{label}</a>
	{/each}
</nav>

{@render children()}
