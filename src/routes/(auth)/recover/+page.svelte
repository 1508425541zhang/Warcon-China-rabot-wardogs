<script lang="ts">
	import { enhance } from '$app/forms';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let busy = $state(false);
</script>

<svelte:head><title>Recover · {data.appName}</title></svelte:head>

<div class="caps text-mist-400">Account recovery</div>
<div class="mt-1 text-[22px] font-semibold tracking-tight">Use your recovery key</div>
<p class="mt-2 text-[13px] leading-relaxed text-mist-400">
	The 40-character key you saved from your account page. It works once: you are signed in and asked
	to set up new sign-in methods straight away. No key? An owner of your organisation can reset your
	sign-in methods from the Users page.
</p>

<form
	method="post"
	class="mt-5 space-y-4"
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
			autocomplete="username"
			spellcheck="false"
			required
			value={form?.username ?? ''}
		/>
	</label>
	<label class="block">
		<span class="field-label">Recovery key</span>
		<input
			class="input font-mono"
			name="key"
			type="text"
			autocomplete="off"
			spellcheck="false"
			placeholder="XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX"
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
	<button class="btn w-full btn-primary" type="submit" disabled={busy}
		>{busy ? 'Checking…' : 'Sign in and recover'}</button
	>
</form>
<p class="note text-center">
	<a href="/sign-in" class="text-accent underline">Back to sign in</a>
</p>
