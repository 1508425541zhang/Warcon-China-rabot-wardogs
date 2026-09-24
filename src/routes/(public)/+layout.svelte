<script lang="ts">
	// The bare public shell: the mark, the server name, the organisation, a Discord invite when
	// the org set one, links between the server's public pages, and one footer line back to the
	// panel. The same width as the panel's pages; one column on a phone, where the link is usually
	// opened from Discord.
	import { page } from '$app/state';
	import Mark from '$lib/components/Mark.svelte';
	import DiscordMark from '$lib/components/DiscordMark.svelte';
	import type { FeatureSet } from '$lib/features';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();
	type Heading = {
		id: string;
		name: string;
		orgName: string;
		discordInviteUrl: string;
		features: FeatureSet;
	};
	let heading = $derived((page.data.heading as Heading | undefined) ?? null);
	let base = $derived(heading ? `/s/${encodeURIComponent(heading.id)}` : '');
	let path = $derived(page.url.pathname);
	const active = (href: string, exact: boolean) =>
		exact ? path === href : path === href || path.startsWith(href + '/');
</script>

<div class="page-x flex min-h-screen flex-col pt-4 pb-6">
	<header class="mb-4 flex flex-wrap items-center gap-3">
		<a href={base || '/'} class="shrink-0" aria-label={data.appName}><Mark size={34} /></a>
		<div class="min-w-0 flex-1">
			<div
				class="truncate font-display text-[20px] leading-tight font-semibold tracking-[0.06em] uppercase"
			>
				{heading?.name ?? data.appName}
			</div>
			{#if heading?.orgName}<div class="truncate caps text-mist-400">{heading.orgName}</div>{/if}
		</div>
		{#if heading?.discordInviteUrl}
			<a
				href={heading.discordInviteUrl}
				target="_blank"
				rel="noopener noreferrer"
				class="btn shrink-0 gap-2"><DiscordMark /> 加入 Discord</a
			>
		{/if}
	</header>
	{#if heading && (heading.features.status || heading.features.leaderboards)}
		<nav class="strip mb-4 gap-1 border-b border-white/8 pb-3" aria-label="公开页面">
			{#if heading.features.status}
				<a href={base} class="tab-link {active(base, true) ? 'tab-link-active' : ''}">实时</a>
				<a
					href="{base}/report"
					class="tab-link {active(`${base}/report`, false) ? 'tab-link-active' : ''}">举报玩家</a
				>
			{/if}
			{#if heading.features.leaderboards}
				<a
					href="{base}/leaderboard"
					class="tab-link {active(`${base}/leaderboard`, false) || active(`${base}/players`, false)
						? 'tab-link-active'
						: ''}">排行榜</a
				>
				<a
					href="{base}/matches"
					class="tab-link {active(`${base}/matches`, false) ? 'tab-link-active' : ''}">比赛记录</a
				>
			{/if}
		</nav>
	{/if}
	<main class="flex-1">{@render children()}</main>
	<footer class="mt-8 border-t border-white/8 pt-4 text-[12px] leading-relaxed text-mist-600">
		技术支持： <a href="/sign-in" class="font-semibold text-accent hover:underline"
			>{data.appName}</a
		>。地图图片 © BULKHEAD，来自 WARDOGS 官方 RCON 控制台；与 BULKHEAD 或 Team17 无关联。
	</footer>
</div>
