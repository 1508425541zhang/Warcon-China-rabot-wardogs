<script lang="ts">
	import { enhance } from '$app/forms';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import DiscordMark from '$lib/components/DiscordMark.svelte';
	import SteamMark from '$lib/components/SteamMark.svelte';
	import { errorMessage } from '$lib/api';
	import { uiMessageZh } from '$lib/ui-message-zh';
	import { passkeysSupported, signInWithPasskey } from '$lib/passkeys';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let busy = $state(false);
	let passkeyError = $state('');
	let canPasskey = $state(false);
	$effect(() => {
		canPasskey = passkeysSupported();
	});
	let STEAM_ERRORS: Record<string, string> = $derived({
		steam_unknown: data.orgSignup
			? '该 Steam 账号尚未关联本站账号。'
			: '该 Steam 账号尚未关联本站账号。请使用组织邀请链接登录，或在账号页面绑定 Steam。',
		steam_disabled: '该账号已停用。',
		steam_state: 'Steam 登录超时或已在其他浏览器中打开，请重试。',
		steam_taken: '该 Steam 账号已关联其他用户。'
	});
	let oauthError = $derived.by(() => {
		const e = page.url.searchParams.get('error') ?? '';
		if (!e) return '';
		if (e === 'discord')
			return data.orgSignup
				? 'Discord 登录失败，请重试或改用其他方式。'
				: 'Discord 登录失败。该账号尚未关联本站账号，请使用组织邀请链接或在账号页面绑定 Discord。';
		if (e.startsWith('steam')) return STEAM_ERRORS[e] ?? 'Steam 登录失败，请重试。';
		return '';
	});
	let deleted = $derived(page.url.searchParams.get('deleted') === '1');
	let action = $derived(
		(name: string) =>
			`?/${name}${data.next === '/' ? '' : `&next=${encodeURIComponent(data.next)}`}`
	);
	// Passwords sit behind a link: shown when the last attempt used one, or when asked for.
	let wantPassword = $state(false);
	let showPassword = $derived(wantPassword || !!form?.username || !canPasskey);

	async function passkey() {
		passkeyError = '';
		busy = true;
		try {
			await signInWithPasskey();
			await goto(data.next, { invalidateAll: true });
		} catch (err) {
			passkeyError = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>登录 · {data.appName}</title></svelte:head>

{#if deleted}
	<div
		class="mb-4 rounded-ctl border border-ok/30 bg-ok/10 px-3 py-2 text-[13px] text-ok"
		role="status"
	>
		账号已删除。
	</div>
{/if}

<div class="space-y-2">
	{#if canPasskey}
		<button class="btn w-full btn-primary" type="button" onclick={passkey} disabled={busy}
			>{busy ? '等待设备确认…' : '使用通行密钥登录'}</button
		>
	{/if}
	<div class="grid gap-2 {data.discord ? 'grid-cols-2' : ''}">
		{#if data.discord}
			<form method="post" action={action('discord')} use:enhance>
				<button class="btn w-full" type="submit"><DiscordMark />Discord</button>
			</form>
		{/if}
		<form method="post" action={action('steam')} use:enhance>
			<button class="btn w-full" type="submit"><SteamMark />Steam</button>
		</form>
	</div>
</div>

{#if passkeyError || oauthError}
	<div
		class="mt-3 rounded-ctl border border-danger/30 bg-danger/12 px-3 py-2 text-[13px] text-danger"
		role="alert"
	>
		{passkeyError || oauthError}
	</div>
{/if}

{#if showPassword}
	<form
		method="post"
		action={action('password')}
		class="mt-4 space-y-4 border-t border-white/8 pt-4"
		use:enhance={() => {
			busy = true;
			return async ({ update }) => {
				await update();
				busy = false;
			};
		}}
	>
		<label class="block">
			<span class="field-label">用户名</span>
			<input
				class="input"
				name="username"
				type="text"
				autocomplete="username webauthn"
				spellcheck="false"
				required
				value={form?.username ?? ''}
			/>
		</label>
		<label class="block">
			<span class="field-label">密码</span>
			<input
				class="input"
				name="password"
				type="password"
				autocomplete="current-password"
				required
			/>
		</label>
		{#if form?.error}
			<div
				class="rounded-ctl border border-danger/30 bg-danger/12 px-3 py-2 text-[13px] text-danger"
				role="alert"
			>
				{uiMessageZh(form.error)}
			</div>
		{/if}
		<button class="btn w-full {canPasskey ? '' : 'btn-primary'}" type="submit" disabled={busy}
			>{busy ? '登录中…' : '使用密码登录'}</button
		>
	</form>
{:else}
	<button
		type="button"
		class="mt-4 block w-full text-center text-[12.5px] text-mist-400 underline hover:text-mist-100"
		onclick={() => (wantPassword = true)}>使用用户名和密码登录</button
	>
{/if}

<p class="note text-center">
	{#if data.orgSignup}
		首次使用？可通过 Discord 或 Steam 直接创建账号，或
		<a href="/sign-up" class="text-accent underline">创建自己的组织</a>。
	{:else if data.discord}
		首次使用？请打开组织提供的邀请链接，并从该页面登录以创建账号。
	{:else}
		首次使用？请打开组织提供的邀请链接。
	{/if}
</p>
<p class="note text-center">
	无法登录？<a href="/recover" class="text-accent underline">使用恢复密钥</a>。
</p>
