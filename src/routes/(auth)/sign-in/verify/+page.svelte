<script lang="ts">
	import { enhance } from '$app/forms';
	import { uiMessageZh } from '$lib/ui-message-zh';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let busy = $state(false);
</script>

<svelte:head><title>二次验证 · {data.appName}</title></svelte:head>

<div class="caps text-mist-400">还需一步</div>
<div class="mt-1 text-[22px] font-semibold tracking-tight">输入验证码</div>
<p class="mt-2 text-[13px] text-mist-400">
	打开验证器应用，输入以下账号的六位验证码： {data.appName}。验证器丢失？也可以使用备用码。
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
		<span class="field-label">代码</span>
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
		信任此浏览器 30 天
	</label>
	{#if form?.error}
		<div
			class="rounded-ctl border border-danger/30 bg-danger/12 px-3 py-2 text-[13px] text-danger"
			role="alert"
		>
			{uiMessageZh(form.error)}
		</div>
	{/if}
	<button class="btn w-full btn-primary" type="submit" disabled={busy}
		>{busy ? 'Checking…' : 'Continue'}</button
	>
</form>
<p class="note text-center">
	<a href="/sign-in" class="text-accent underline">重新开始</a>
</p>
