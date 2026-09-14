<script lang="ts">
	import { page } from '$app/state';
	import { health, identity, setHealth, throttled } from '$lib/health.svelte';
	import Pulse from '$lib/components/Pulse.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import { toast } from '$lib/toast.svelte';
	import { sponsor, loadSponsor } from '$lib/sponsor.svelte';
	import { fmtUptime, restartWindow, RESTART_AFTER_HOURS, RESTART_SOON_MS } from '$lib/uptime';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	const TABS = [
		['', 'Overview'],
		['/players', 'Players'],
		['/bans', 'Bans'],
		['/slots', 'Reserved slots'],
		['/rotation', 'Map rotation'],
		['/config', 'Configuration'],
		['/automation', 'Automation'],
		['/analytics', 'Analytics'],
		['/log', 'Server log']
	] as const;
	// Discord webhooks are an org owner's to manage, so the tab shows for them alone.
	let visibleTabs = $derived(
		data.server.manager ? [...TABS, ['/discord', 'Discord'] as const] : [...TABS]
	);
	let base = $derived(`/server/${encodeURIComponent(data.server.id)}`);
	let current = $derived(page.url.pathname.slice(base.length) || '');
	// A dossier (/players/<steamId>) keeps the Players tab lit.
	const isCurrent = (path: string) =>
		current === path || (path !== '' && current.startsWith(path + '/'));

	$effect(() => {
		setHealth(data.server.id, data.reachable);
	});

	// The stream may learn the id after the page loaded; the load's answer is the fallback.
	let ident = $derived(identity[data.server.id] ?? data.identity);
	async function copyId(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast('Join code copied.', 'ok');
		} catch {
			window.prompt('Copy the join code:', text);
		}
	}
	let live = $derived(health[data.server.id]);
	// The uptime counts up on its own between looks: a minute clock, only while the page is open.
	let now = $state(Date.now());
	$effect(() => {
		const t = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(t);
	});
	let restart = $derived(
		live === false ? null : restartWindow(ident.startedAt, RESTART_AFTER_HOURS, now)
	);
	// The banner the server advertises to the game's browser, beside the card as the official console has it.
	let banner = $derived(sponsor[data.server.id] ?? '');
	$effect(() => {
		if (data.reachable) void loadSponsor(data.server.id);
	});
	let slowed = $derived(!!throttled[data.server.id]);

	// On phones the tab row scrolls sideways; keep the active tab in view after navigating.
	let tabs = $state<HTMLElement>();
	$effect(() => {
		void current;
		const active = tabs?.querySelector<HTMLElement>('.tab-link-active');
		if (!active || !tabs || tabs.scrollWidth <= tabs.clientWidth) return;
		const left = active.offsetLeft - (tabs.clientWidth - active.offsetWidth) / 2;
		tabs.scrollTo({ left, behavior: 'smooth' });
	});
</script>

<svelte:head><title>{data.server.name} · {data.appName}</title></svelte:head>

<div class="mb-4 rise rounded-card border border-l-[3px] border-black border-l-accent bg-ink-900">
	<div class="flex flex-wrap items-start gap-3 px-5 py-4">
		<div class="min-w-0 grow">
			<h1 class="flex flex-wrap items-center gap-2 text-xl font-semibold tracking-tight">
				<span class="truncate">{data.server.name}</span>
				<RoleBadge role={data.server.roleName} />
				{#if data.server.demo}<Badge tone="info">demo</Badge>{/if}
			</h1>
			<div class="mt-1.5 flex flex-wrap items-center gap-x-5 gap-y-1 text-[12.5px] text-mist-400">
				<span class="inline-flex items-baseline gap-1.5">
					<span class="caps">Address</span>
					<span class="font-mono text-mist-200">{data.server.host}:{data.server.port}</span>
				</span>
				{#if ident.gameServerId}
					<button
						type="button"
						class="group inline-flex cursor-pointer items-baseline gap-1.5 text-left"
						title="Click to copy"
						onclick={() => copyId(ident.gameServerId)}
					>
						<span class="caps whitespace-nowrap">Join code</span>
						<span class="text-left font-mono break-all text-mist-200 group-hover:text-white"
							>{ident.gameServerId}</span
						>
						<span class="caps text-mist-600 group-hover:text-mist-300">copy</span>
					</button>
				{/if}
				{#if restart}
					<span class="inline-flex flex-wrap items-baseline gap-1.5">
						<span class="caps">Up</span>
						<span class="text-mist-200">{fmtUptime(restart.upMs)}</span>
						{#if restart.due}
							<Badge tone="warn">restarts after this round</Badge>
						{:else if restart.untilDueMs !== null && restart.untilDueMs <= RESTART_SOON_MS}
							<span class="text-warn">restart window in {fmtUptime(restart.untilDueMs)}</span>
						{/if}
					</span>
				{/if}
			</div>
			{#if data.server.notes}
				<p class="mt-1.5 line-clamp-2 text-[12.5px] text-mist-400">{data.server.notes}</p>
			{/if}
		</div>
		{#if banner}
			{#key banner}
				<img
					src={banner}
					alt="Server banner"
					class="h-12 w-auto max-w-[192px] rounded border border-black object-cover"
					loading="lazy"
					referrerpolicy="no-referrer"
					onerror={(e) => ((e.currentTarget as HTMLImageElement).hidden = true)}
				/>
			{/key}
		{/if}
		<span
			class="mt-1 inline-flex items-center gap-2 text-[12.5px] {live === false
				? 'text-danger'
				: slowed
					? 'text-warn'
					: 'text-mist-400'}"
			title={slowed
				? 'The game server asked the panel to slow down (its per-address request limit); the next look waits for the time it gave.'
				: undefined}
		>
			<Pulse ok={live} />
			{live === true
				? slowed
					? 'rate limited, retrying'
					: 'live'
				: live === false
					? 'unreachable'
					: 'connecting…'}
		</span>
	</div>
</div>

<nav
	bind:this={tabs}
	class="strip mb-5 gap-1 border-b border-white/8 pb-3"
	aria-label="Server sections"
>
	{#each visibleTabs as [path, label] (path)}
		<a href="{base}{path}" class="tab-link {isCurrent(path) ? 'tab-link-active' : ''}">{label}</a>
	{/each}
</nav>

{#if data.reachable}
	{#key data.server.id}
		{@render children()}
	{/key}
{:else}
	<div class="callout border-danger/30 bg-danger/12">
		<b>Cannot reach this server.</b>
		{data.problem}
	</div>
	<p class="note">
		Check the host, port and RCON password under Servers, and that the listener is bound to a
		reachable address.
	</p>
{/if}
