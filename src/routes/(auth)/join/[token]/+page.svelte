<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import RegisterForm from '$lib/components/RegisterForm.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import { uiMessageZh } from '$lib/ui-message-zh';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let busy = $state(false);
	let oauthError = $derived.by(() => {
		const e = page.url.searchParams.get('error') ?? '';
		if (e === 'discord') return 'Discord 登录失败，请重试。';
		if (e === 'steam_disabled') return '此账号已停用。';
		if (e.startsWith('steam')) return 'Steam 登录失败，请重试。';
		return '';
	});
	let signInHref = $derived(`/sign-in?next=${encodeURIComponent(page.url.pathname)}`);
</script>

<svelte:head><title>加入组织 · {data.appName}</title></svelte:head>

{#if !data.valid || !data.org}
	<div class="caps text-mist-400">邀请链接</div>
	<p class="mt-2 text-[15px]">{data.problem}</p>
	<p class="note">请联系邀请人获取新链接。</p>
	{#if data.user}
		<a href="/" class="mt-4 btn w-full">返回总览</a>
	{:else}
		<a href="/sign-in" class="mt-4 btn w-full">登录</a>
	{/if}
{:else}
	<div class="caps text-mist-400">你已受邀加入</div>
	<div class="mt-1 text-[22px] font-semibold tracking-tight">{data.org.name}</div>
	<div class="mt-3 flex flex-wrap items-center gap-2 text-[13px] text-mist-400">
		<span>以</span>
		<RoleBadge role={data.orgRole ?? 'member'} />
		{#if data.serverRole}
			<span>及</span>
			<RoleBadge role={data.serverRole} />
			<span>访问该组织的服务器</span>
		{:else}
			<span>（随后由所有者授予服务器权限）</span>
		{/if}
	</div>

	{#if form?.error || oauthError}
		<div
			class="mt-4 rounded-ctl border border-danger/30 bg-danger/12 px-3 py-2 text-[13px] text-danger"
			role="alert"
		>
			{uiMessageZh(form?.error || oauthError)}
		</div>
	{/if}

	{#if data.user}
		<div class="mt-5 kv">
			<span class="text-mist-400">当前登录账号</span>
			<span
				>{data.user.name} <span class="font-mono text-mist-400">@{data.user.username}</span></span
			>
		</div>
		{#if data.alreadyMember}
			<p class="mt-4 text-[13.5px]">你已经是以下组织的成员： {data.org.name}.</p>
			<a href="/" class="mt-3 btn w-full btn-primary">打开总览</a>
		{:else}
			<form
				method="post"
				action="?/join"
				class="mt-4"
				use:enhance={() => {
					busy = true;
					return async ({ update }) => {
						await update();
						busy = false;
					};
				}}
			>
				<button class="btn w-full btn-primary" type="submit" disabled={busy}
					>{busy ? 'Joining…' : `Join ${data.org.name}`}</button
				>
			</form>
			<form method="post" action="/sign-out" class="mt-3 text-center">
				<button type="submit" class="text-[12.5px] text-mist-400 underline hover:text-mist-100"
					>不是你？退出登录</button
				>
			</form>
		{/if}
	{:else}
		<RegisterForm
			discord={data.discord}
			steam
			discordAction="?/discord"
			steamAction="?/steam"
			registerAction="?/register"
			{signInHref}
			passkeyLabel="使用通行密钥创建账号"
			registerLabel="创建账号并加入"
			turnstileSiteKey={data.turnstileSiteKey}
			invite={page.params.token}
			{form}
			onPasskeyDone={() => invalidateAll()}
		/>
	{/if}
{/if}
