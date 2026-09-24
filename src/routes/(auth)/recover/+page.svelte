<script lang="ts">
	import { enhance } from '$app/forms';
	import { uiMessageZh } from '$lib/ui-message-zh';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let busy = $state(false);
</script>

<svelte:head><title>恢复账号 · {data.appName}</title></svelte:head>

<div class="caps text-mist-400">账号恢复</div>
<div class="mt-1 text-[22px] font-semibold tracking-tight">使用恢复密钥</div>
<p class="mt-2 text-[13px] leading-relaxed text-mist-400">
	输入你在账号页面保存的 40
	位恢复密钥。密钥只能使用一次；登录后系统会立即要求你设置新的登录方式。没有密钥？请联系组织所有者，在“用户”页面重置你的登录方式。
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
		<span class="field-label">用户名</span>
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
		<span class="field-label">恢复密钥</span>
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
			{uiMessageZh(form.error)}
		</div>
	{/if}
	<button class="btn w-full btn-primary" type="submit" disabled={busy}
		>{busy ? 'Checking…' : '登录并恢复账号'}</button
	>
</form>
<p class="note text-center">
	<a href="/sign-in" class="text-accent underline">返回登录</a>
</p>
