<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import RegisterForm from '$lib/components/RegisterForm.svelte';
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
	let canCreate = $derived(data.remaining === null || data.remaining > 0);
	// Only the create action echoes the org name back; the register/discord actions do not.
	let orgName = $derived((form as { orgName?: string } | null)?.orgName ?? '');
</script>

<svelte:head><title>创建组织 · {data.appName}</title></svelte:head>

<div class="caps text-mist-400">设置战队或社区</div>
<div class="mt-1 text-[22px] font-semibold tracking-tight">创建组织</div>
<p class="mt-2 text-[13px] text-mist-400">
	你将成为组织所有者：添加服务器、为 Discord 社区创建邀请链接，并设置成员角色。
</p>

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
		<span>{data.user.name} <span class="font-mono text-mist-400">@{data.user.username}</span></span>
	</div>
	{#if canCreate}
		<form
			method="post"
			action="?/create"
			class="mt-4 space-y-3"
			use:enhance={() => {
				busy = true;
				return async ({ update }) => {
					await update({ reset: false });
					busy = false;
				};
			}}
		>
			<label class="block"
				><span class="field-label">组织名称</span><input
					class="input"
					name="orgName"
					type="text"
					placeholder="战队或社区名称"
					minlength="2"
					maxlength="60"
					required
					value={orgName}
				/></label
			>
			<button class="btn w-full btn-primary" type="submit" disabled={busy}
				>{busy ? 'Creating…' : '创建组织'}</button
			>
		</form>
		<a
			href="/"
			class="mt-3 block text-center text-[12.5px] text-mist-400 underline hover:text-mist-100"
			>返回总览</a
		>
	{:else}
		<p class="mt-4 text-[13.5px]">你已达到可创建的组织数量上限。如需更多，请联系平台所有者。</p>
		<a href="/orgs" class="mt-3 btn w-full">你的组织</a>
	{/if}
{:else}
	<RegisterForm
		discord={data.discord}
		steam
		discordAction="?/discord"
		steamAction="?/steam"
		registerAction="?/register"
		signInHref="/sign-in?next=%2Fsign-up"
		registerLabel="Create account"
		turnstileSiteKey={data.turnstileSiteKey}
		{form}
		onPasskeyDone={() => invalidateAll()}
	/>
{/if}
