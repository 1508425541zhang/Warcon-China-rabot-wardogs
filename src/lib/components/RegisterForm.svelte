<script lang="ts">
	// Account creation for pages where a visitor without an account may make one (first-run setup,
	// invite links, organisation sign-up). Providers first, then a passkey; a password is the
	// fallback behind a link, not the default.
	import { enhance } from '$app/forms';
	import { untrack } from 'svelte';
	import { resetTurnstile, turnstile } from '$lib/turnstile';
	import { addPasskey, passkeysSupported, suggestPasskeyName } from '$lib/passkeys';
	import { errorMessage } from '$lib/api';
	import DiscordMark from './DiscordMark.svelte';
	import SteamMark from './SteamMark.svelte';

	let {
		discord = false,
		steam = false,
		discordAction = '',
		steamAction = '',
		registerAction,
		signInHref,
		discordLabel = '使用 Discord 继续',
		steamLabel = '使用 Steam 继续',
		passkeyLabel = '使用通行密钥创建账号',
		registerLabel = '创建账号',
		turnstileSiteKey = null,
		invite = '',
		tokenRequired = false,
		form,
		onPasskeyDone
	}: {
		discord?: boolean;
		steam?: boolean;
		discordAction?: string;
		steamAction?: string;
		registerAction: string;
		signInHref: string | null;
		discordLabel?: string;
		steamLabel?: string;
		passkeyLabel?: string;
		registerLabel?: string;
		/** Cloudflare Turnstile site key; null renders no challenge */
		turnstileSiteKey?: string | null;
		/** invite link token: lets a passkey sign-up through when open sign-up is off */
		invite?: string;
		/** first-run setup: ask for the SETUP_TOKEN */
		tokenRequired?: boolean;
		form: { error?: string; username?: string; displayName?: string } | null | undefined;
		/** called once a passkey sign-up has signed the new account in */
		onPasskeyDone: () => void | Promise<void>;
	} = $props();

	let wantPassword = $state(false);
	// Open the password form when a previous password attempt failed.
	let showPassword = $derived(wantPassword || !!form?.username);
	let busy = $state(false);
	let username = $state(untrack(() => form?.username ?? ''));
	let displayName = $state(untrack(() => form?.displayName ?? ''));
	let token = $state('');
	let error = $state('');
	let formEl: HTMLFormElement | undefined = $state();
	let canPasskey = $state(false);
	$effect(() => {
		canPasskey = passkeysSupported();
	});

	async function passkey() {
		error = '';
		if (!formEl?.reportValidity()) return;
		busy = true;
		try {
			const turnstileToken =
				(formEl.elements.namedItem('cf-turnstile-response') as HTMLInputElement | null)?.value ??
				'';
			await addPasskey(suggestPasskeyName(), {
				username: username.trim(),
				displayName: displayName.trim(),
				token: token || undefined,
				invite: invite || undefined,
				turnstile: turnstileToken || undefined
			});
			await onPasskeyDone();
		} catch (err) {
			error = errorMessage(err);
			resetTurnstile();
		} finally {
			busy = false;
		}
	}
</script>

{#if discord || steam}
	<div class="mt-5 grid gap-2 {discord && steam ? 'grid-cols-2' : ''}">
		{#if discord}
			<form method="post" action={discordAction} use:enhance>
				<button class="btn w-full" type="submit"><DiscordMark />{discordLabel}</button>
			</form>
		{/if}
		{#if steam}
			<form method="post" action={steamAction} use:enhance>
				<button class="btn w-full" type="submit"><SteamMark />{steamLabel}</button>
			</form>
		{/if}
	</div>
	<p class="mt-2 text-center text-[12.5px] text-mist-400">
		使用 Discord 或 Steam 身份创建账号，无需密码。
	</p>
	<div class="my-4 flex items-center gap-3 caps text-[11px] text-mist-600">
		<span class="h-px flex-1 bg-white/8"></span>或使用用户名<span class="h-px flex-1 bg-white/8"
		></span>
	</div>
{/if}

<form
	bind:this={formEl}
	method="post"
	action={registerAction}
	class="space-y-3 {discord || steam ? '' : 'mt-5'}"
	use:enhance={() => {
		busy = true;
		return async ({ update, result }) => {
			await update({ reset: false });
			if (result.type !== 'redirect') resetTurnstile();
			busy = false;
		};
	}}
>
	<label class="block"
		><span class="field-label">用户名</span><input
			class="input"
			name="username"
			type="text"
			autocomplete="username"
			spellcheck="false"
			minlength="2"
			maxlength="32"
			required
			bind:value={username}
		/></label
	>
	<label class="block"
		><span class="field-label">显示名称（选填）</span><input
			class="input"
			name="displayName"
			type="text"
			autocomplete="name"
			maxlength="80"
			bind:value={displayName}
		/></label
	>
	{#if tokenRequired}
		<label class="block">
			<span class="field-label">初始化令牌</span>
			<input
				class="input"
				name="token"
				type="password"
				autocomplete="off"
				required
				bind:value={token}
			/>
			<p class="note">部署时设置的 SETUP_TOKEN 密钥。</p>
		</label>
	{/if}
	{#if turnstileSiteKey}
		<div use:turnstile={turnstileSiteKey}></div>
	{/if}

	{#if canPasskey}
		<button type="button" class="btn w-full btn-primary" onclick={passkey} disabled={busy}
			>{busy ? '处理中…' : passkeyLabel}</button
		>
		<p class="text-center text-[12.5px] text-mist-400">
			通行密钥使用设备解锁方式（面容、指纹或 Windows Hello），无需记忆密码。
		</p>
	{/if}

	{#if showPassword}
		<label class="block border-t border-white/8 pt-3"
			><span class="field-label">密码（至少 10 个字符）</span><input
				class="input"
				name="password"
				type="password"
				autocomplete="new-password"
				minlength="10"
				required
			/></label
		>
		<p class="text-[12.5px] text-mist-400">
			使用密码后，请尽快在账号页面设置验证器和第二种登录方式。
		</p>
		<button class="btn w-full {canPasskey ? '' : 'btn-primary'}" type="submit" disabled={busy}
			>{busy ? '创建中…' : registerLabel}</button
		>
	{:else}
		<button
			type="button"
			class="block w-full text-center text-[12.5px] text-mist-400 underline hover:text-mist-100"
			onclick={() => (wantPassword = true)}>改用密码</button
		>
	{/if}

	{#if error || form?.error}
		<div
			class="rounded-ctl border border-danger/30 bg-danger/12 px-3 py-2 text-[13px] text-danger"
			role="alert"
		>
			{error || form?.error}
		</div>
	{/if}
</form>

{#if signInHref}
	<p class="note text-center">
		已有账号？<a href={signInHref} class="text-accent underline">登录</a>。
	</p>
{/if}
