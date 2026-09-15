<script lang="ts">
	import { enhance } from '$app/forms';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import DiscordMark from '$lib/components/DiscordMark.svelte';
	import SteamMark from '$lib/components/SteamMark.svelte';
	import { errorMessage } from '$lib/api';
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
			? 'That Steam account is not linked to a panel account here.'
			: 'That Steam account is not linked to a panel account here: open an invite link from your organisation and sign in with Steam there, or link Steam from your account page.',
		steam_disabled: 'This account is disabled.',
		steam_state: 'The Steam sign-in took too long or was opened in another browser. Try again.',
		steam_taken: 'That Steam account is already linked to another user.'
	});
	let oauthError = $derived.by(() => {
		const e = page.url.searchParams.get('error') ?? '';
		if (!e) return '';
		if (e === 'discord')
			return data.orgSignup
				? 'Discord sign-in failed. Try again, or use another method.'
				: 'Discord sign-in failed. That Discord account is not linked to a panel account here: open an invite link from your organisation, or link Discord from your account page.';
		if (e.startsWith('steam')) return STEAM_ERRORS[e] ?? 'Steam sign-in failed. Try again.';
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

<svelte:head><title>Sign in · {data.appName}</title></svelte:head>

{#if deleted}
	<div
		class="mb-4 rounded-ctl border border-ok/30 bg-ok/10 px-3 py-2 text-[13px] text-ok"
		role="status"
	>
		Your account has been deleted.
	</div>
{/if}

<div class="space-y-2">
	{#if canPasskey}
		<button class="btn w-full btn-primary" type="button" onclick={passkey} disabled={busy}
			>{busy ? 'Waiting for your device…' : 'Sign in with a passkey'}</button
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
			<span class="field-label">Username</span>
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
			<span class="field-label">Password</span>
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
				{form.error}
			</div>
		{/if}
		<button class="btn w-full {canPasskey ? '' : 'btn-primary'}" type="submit" disabled={busy}
			>{busy ? 'Signing in…' : 'Sign in with password'}</button
		>
	</form>
{:else}
	<button
		type="button"
		class="mt-4 block w-full text-center text-[12.5px] text-mist-400 underline hover:text-mist-100"
		onclick={() => (wantPassword = true)}>Sign in with a username and password</button
	>
{/if}

<p class="note text-center">
	{#if data.orgSignup}
		New here? Discord or Steam creates your account on the spot, or
		<a href="/sign-up" class="text-accent underline">create your own organisation</a>.
	{:else if data.discord}
		New here? Open the invite link from your organisation and sign in there: it creates your
		account.
	{:else}
		New here? Open the invite link from your organisation.
	{/if}
</p>
<p class="note text-center">
	Lost every way in? <a href="/recover" class="text-accent underline">Use your recovery key</a>.
</p>
