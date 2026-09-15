<script lang="ts">
	import { enhance } from '$app/forms';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let busy = $state(false);
</script>

<svelte:head><title>Second step · {data.appName}</title></svelte:head>

<div class="caps text-mist-400">One more step</div>
<div class="mt-1 text-[22px] font-semibold tracking-tight">Enter your code</div>
<p class="mt-2 text-[13px] text-mist-400">
	Open your authenticator app and type the six-digit code for {data.appName}. Lost the app? A backup
	code works here too.
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
		<span class="field-label">Code</span>
		<input
			class="input font-mono text-[18px] tracking-[0.3em]"
			name="code"
			type="text"
			inputmode="numeric"
			autocomplete="one-time-code"
			spellcheck="false"
			required
		/>
	</label>
	<label class="flex items-center gap-2 text-[13px] text-mist-400">
		<input type="checkbox" name="trust" class="accent-brass-400" />
		Trust this browser for 30 days
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
		>{busy ? 'Checking…' : 'Continue'}</button
	>
</form>
<p class="note text-center">
	<a href="/sign-in" class="text-accent underline">Start over</a>
</p>
