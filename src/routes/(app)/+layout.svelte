<script lang="ts">
	import { page } from '$app/state';
	import { afterNavigate, invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import Mark from '$lib/components/Mark.svelte';
	import Pulse from '$lib/components/Pulse.svelte';
	import { health } from '$lib/health.svelte';
	import { initials } from '$lib/format';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	let currentId = $derived(
		page.route.id?.includes('/server/[id]') ? (page.params.id ?? null) : null
	);
	// A server outside the org scope still opens by link: the server layout supplies it.
	let current = $derived(
		data.servers.find((s) => s.id === currentId) ??
			(currentId ? (page.data.server as (typeof data.servers)[number] | undefined) : undefined)
	);
	// Group the switcher by org once the user can see more than one.
	let multiOrg = $derived(new Set(data.servers.map((s) => s.orgId)).size > 1);
	let switcherOpen = $state(false);
	let scopeOpen = $state(false);
	let userOpen = $state(false);

	// Type-to-filter once the switcher is long enough to need it.
	let query = $state('');
	let searchable = $derived(data.servers.length > 8);
	let listed = $derived.by(() => {
		const needle = query.trim().toLowerCase();
		if (!needle) return data.servers;
		return data.servers.filter((s) =>
			`${s.name} ${s.host}:${s.port} ${s.orgName}`.toLowerCase().includes(needle)
		);
	});

	// The organisation scope: which org's servers the switcher, dashboard and Servers page show.
	let scopeBusy = $state(false);
	async function setScope(orgId: string | null) {
		scopeOpen = false;
		switcherOpen = false;
		if ((data.scope?.id ?? null) === orgId) return;
		scopeBusy = true;
		try {
			await api('PUT', '/api/scope', { orgId });
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			scopeBusy = false;
		}
	}

	const isActive = (path: string) =>
		path === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(path);
	const closeAll = () => {
		switcherOpen = false;
		scopeOpen = false;
		userOpen = false;
		query = '';
	};
	afterNavigate(closeAll);
</script>

<svelte:window onclick={closeAll} onkeydown={(e) => e.key === 'Escape' && closeAll()} />

<header class="sticky top-0 z-30 border-b border-black bg-ink-900">
	<div class="page-x flex h-[58px] items-center gap-3">
		<a
			href="/"
			class="flex shrink-0 items-center gap-2.5 font-semibold tracking-tight"
			aria-label="{data.appName} dashboard"
		>
			<Mark />
			<span
				class="font-display text-[19px] font-semibold tracking-[0.1em] uppercase {current
					? 'hidden sm:inline'
					: ''}">{data.appName}</span
			>
			<span class="hidden caps text-mist-600 sm:inline">RCON</span>
		</a>

		{#if data.orgs.length > 1}
			<!-- Phones get the same choice inside the server switcher instead. -->
			<div class="relative ml-1 hidden shrink-0 sm:block">
				<button
					type="button"
					class="btn max-w-[110px] gap-1.5 pr-2.5 sm:max-w-[200px]"
					title="组织范围"
					aria-haspopup="menu"
					aria-expanded={scopeOpen}
					disabled={scopeBusy}
					onclick={(e) => {
						e.stopPropagation();
						userOpen = false;
						switcherOpen = false;
						scopeOpen = !scopeOpen;
					}}
				>
					<span class="truncate {data.scope ? '' : 'text-mist-400'}"
						>{data.scope ? data.scope.name : '所有组织'}</span
					>
					<span class="text-[10px] text-mist-600">▼</span>
				</button>
				{#if scopeOpen}
					<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
					<div class="menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()}>
						<div class="px-3 pt-2 pb-1 caps text-mist-600">显示以下组织的服务器</div>
						<button
							type="button"
							class="menu-item {data.scope ? '' : 'border-accent! bg-accent/10 text-accent'}"
							role="menuitemradio"
							aria-checked={!data.scope}
							onclick={() => setScope(null)}>全部组织</button
						>
						{#each data.orgs as o (o.id)}
							<button
								type="button"
								class="menu-item {data.scope?.id === o.id
									? 'border-accent! bg-accent/10 text-accent'
									: ''}"
								role="menuitemradio"
								aria-checked={data.scope?.id === o.id}
								onclick={() => setScope(o.id)}
							>
								<span class="truncate">{o.name}</span>
								{#if o.id === data.user.defaultOrgId}<span class="ml-auto text-[11px] text-mist-600"
										>默认</span
									>{/if}
							</button>
						{/each}
						<div class="my-1.5 border-t border-white/8"></div>
						<a href="/account" class="menu-item text-mist-400" role="menuitem" onclick={closeAll}
							>设置默认值…</a
						>
					</div>
				{/if}
			</div>
		{/if}

		<div class="relative ml-1 min-w-0 flex-1 sm:flex-none">
			<button
				type="button"
				class="btn max-w-full gap-2 pr-2.5 text-[13px] font-medium tracking-normal normal-case sm:max-w-[260px]"
				aria-haspopup="menu"
				aria-expanded={switcherOpen}
				onclick={(e) => {
					e.stopPropagation();
					userOpen = false;
					scopeOpen = false;
					switcherOpen = !switcherOpen;
				}}
			>
				<Pulse ok={current ? health[current.id] : undefined} />
				<span class="truncate">{current ? current.name : '选择服务器'}</span>
				<span class="text-[10px] text-mist-600">▼</span>
			</button>
			{#if switcherOpen}
				<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
				<div class="menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()}>
					{#if searchable}
						<div class="p-1 pb-1.5">
							<input
								class="input py-1.5 text-[13px]"
								type="search"
								placeholder="查找服务器…"
								bind:value={query}
								aria-label="查找服务器"
								{@attach (el) => el.focus()}
							/>
						</div>
					{/if}
					{#each listed as s, i (s.id)}
						{#if multiOrg && (i === 0 || listed[i - 1].orgId !== s.orgId)}
							<div class="px-3 pt-2 pb-1 caps text-mist-600">{s.orgName}</div>
						{/if}
						<a
							href="/server/{encodeURIComponent(s.id)}"
							class="menu-item {s.id === currentId
								? 'border-accent! bg-accent/10 text-accent'
								: ''}"
							role="menuitem"
							onclick={closeAll}
						>
							<Pulse ok={health[s.id]} />
							<span class="truncate">{s.name}</span>
							<span class="ml-auto text-[11px] text-mist-600">{s.roleName}</span>
						</a>
					{:else}
						<div class="px-3 py-2 text-[12.5px] text-mist-400">
							{#if query.trim()}
								没有符合条件的服务器。
							{:else if data.scope}
								{data.scope.name} 暂无服务器。
							{:else}
								{data.canManage ? '暂无服务器。' : '尚无向你共享的服务器。'}
							{/if}
						</div>
					{/each}
					{#if data.canManage}
						<div class="my-1.5 border-t border-white/8"></div>
						<a href="/servers" class="menu-item text-mist-400" role="menuitem" onclick={closeAll}
							>管理服务器…</a
						>
					{/if}
					{#if data.orgs.length > 1}
						<div class="sm:hidden">
							<div class="my-1.5 border-t border-white/8"></div>
							<div class="px-3 pt-1 pb-1 caps text-mist-600">显示以下组织的服务器</div>
							<button
								type="button"
								class="menu-item {data.scope ? '' : 'border-accent! bg-accent/10 text-accent'}"
								role="menuitemradio"
								aria-checked={!data.scope}
								onclick={() => setScope(null)}>全部组织</button
							>
							{#each data.orgs as o (o.id)}
								<button
									type="button"
									class="menu-item {data.scope?.id === o.id
										? 'border-accent! bg-accent/10 text-accent'
										: ''}"
									role="menuitemradio"
									aria-checked={data.scope?.id === o.id}
									onclick={() => setScope(o.id)}><span class="truncate">{o.name}</span></button
								>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>

		<nav class="ml-2 hidden items-center gap-1 md:flex" aria-label="主导航">
			<a href="/" class="nav-pill {isActive('/') ? 'nav-pill-active' : ''}">概览</a>
			<a href="/audit" class="nav-pill {isActive('/audit') ? 'nav-pill-active' : ''}">审计</a>
			{#if data.canManage}
				<a href="/servers" class="nav-pill {isActive('/servers') ? 'nav-pill-active' : ''}"
					>服务器</a
				>
			{/if}
			<a href="/orgs" class="nav-pill {isActive('/orgs') ? 'nav-pill-active' : ''}">组织</a>
			{#if data.user.role === 'owner'}
				<a href="/admin" class="nav-pill {isActive('/admin') ? 'nav-pill-active' : ''}">站点管理</a>
			{/if}
		</nav>

		<div class="relative ml-auto shrink-0">
			<button
				type="button"
				class="flex h-9 w-9 items-center justify-center overflow-hidden rounded-ctl border border-black bg-accent text-[12px] font-bold text-ink-950"
				title="@{data.user.username}"
				aria-haspopup="menu"
				aria-expanded={userOpen}
				onclick={(e) => {
					e.stopPropagation();
					switcherOpen = false;
					userOpen = !userOpen;
				}}
			>
				{#if data.user.image}
					<img
						src={data.user.image}
						alt=""
						class="h-full w-full object-cover"
						referrerpolicy="no-referrer"
					/>
				{:else}
					{initials(data.user.name || data.user.username)}
				{/if}
			</button>
			{#if userOpen}
				<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
				<div
					class="menu right-0 left-auto max-sm:left-3"
					role="menu"
					tabindex="-1"
					onclick={(e) => e.stopPropagation()}
				>
					<div class="px-3 py-2">
						<div class="font-semibold">{data.user.name || data.user.username}</div>
						<div class="font-mono text-[12px] text-mist-400">
							@{data.user.username} · {data.user.role}
						</div>
					</div>
					<div class="my-1.5 border-t border-white/8"></div>
					<nav class="md:hidden" aria-label="移动端主导航">
						<a href="/" class="menu-item" role="menuitem">概览</a>
						<a href="/audit" class="menu-item" role="menuitem">审计</a>
						{#if data.canManage}
							<a href="/servers" class="menu-item" role="menuitem">服务器</a>
						{/if}
						<a href="/orgs" class="menu-item" role="menuitem">组织</a>
						{#if data.user.role === 'owner'}
							<a href="/admin" class="menu-item" role="menuitem">站点管理</a>
						{/if}
						<div class="my-1.5 border-t border-white/8"></div>
					</nav>
					<a href="/account" class="menu-item" role="menuitem">账号与会话</a>
					<form method="post" action="/sign-out">
						<button type="submit" class="menu-item text-mist-400" role="menuitem">退出登录</button>
					</form>
				</div>
			{/if}
		</div>
	</div>
</header>

{#if !data.user.authComplete && data.authPolicy.nudge && !data.user.apiKey && !page.url.pathname.startsWith('/account')}
	<div class="page-x pt-4">
		<a
			href="/account?enrol=1"
			class="flex flex-wrap items-center justify-between gap-2 rounded-ctl border border-warn/30 bg-warn/10 px-3 py-2 text-[13px] hover:bg-warn/15"
		>
			<span>
				<span class="font-medium">请完成登录方式设置。</span>
				请至少设置两种登录方式；使用密码时还应启用身份验证器，避免设备丢失后无法登录。
			</span>
			<span class="text-mist-400">
				{#if data.authPolicy.enforced && data.enrolment.daysLeft !== null}
					剩余 {data.enrolment.daysLeft} 天 →
				{:else if data.authPolicy.enforced}
					必须完成 →
				{:else}
					建议完成 →
				{/if}
			</span>
		</a>
	</div>
{/if}

<main class="page-x py-6">
	{@render children()}
</main>
<footer class="page-x pb-6 text-[12px] text-mist-600">
	地图图片 © BULKHEAD，来自 WARDOGS 官方 RCON 控制台。Warcon 是社区工具，与 BULKHEAD 或 Team17
	无关联。
</footer>
