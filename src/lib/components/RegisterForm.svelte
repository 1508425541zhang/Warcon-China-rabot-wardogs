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
		discordLabel = 'Continue with Discord',
		steamLabel = 'Continue with Steam',
		passkeyLabel = 'Create account with a passkey',
		registerLabel = 'Create account',
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
		Your Discord or Steam identity becomes the account, no password involved.
	</p>
	<div class="my-4 flex items-center gap-3 caps text-[11px] text-mist-600">
		<span class="h-px flex-1 bg-white/8"></span>or with a username<span
			class="h-px flex-1 bg-white/8"
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
		><span class="field-label">Username</span><input
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
		><span class="field-label">Display name (optional)</span><input
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
			<span class="field-label">Setup token</span>
			<input
				class="input"
				name="token"
				type="password"
				autocomplete="off"
				required
				bind:value={token}
			/>
			<p class="note">The SETUP_TOKEN secret set by whoever deployed this panel.</p>
		</label>
	{/if}
	{#if turnstileSiteKey}
		<div use:turnstile={turnstileSiteKey}></div>
	{/if}

	{#if canPasskey}
		<button type="button" class="btn w-full btn-primary" onclick={passkey} disabled={busy}
			>{busy ? 'Working…' : passkeyLabel}</button
		>
		<p class="text-center text-[12.5px] text-mist-400">
			A passkey is your device's own lock (Face ID, fingerprint, Windows Hello): nothing to
			remember, nothing to phish.
		</p>
	{/if}

	{#if showPassword}
		<label class="block border-t border-white/8 pt-3"
			><span class="field-label">Password (10+ characters)</span><input
				class="input"
				name="password"
				type="password"
				autocomplete="new-password"
				minlength="10"
				required
			/></label
		>
		<p class="text-[12.5px] text-mist-400">
			Passwords need an authenticator app and a second way in before long; the account page walks
			you through it.
		</p>
		<button class="btn w-full {canPasskey ? '' : 'btn-primary'}" type="submit" disabled={busy}
			>{busy ? 'Creating…' : registerLabel}</button
		>
	{:else}
		<button
			type="button"
			class="block w-full text-center text-[12.5px] text-mist-400 underline hover:text-mist-100"
			onclick={() => (wantPassword = true)}>Use a password instead</button
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
		Already have an account? <a href={signInHref} class="text-accent underline">Sign in</a>.
	</p>
{/if}
