<script lang="ts">
	import { api, errorMessage } from '$lib/api';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let target = $state('');
	let reason = $state('');
	let busy = $state(false);
	let success = $state(false);
	let failure = $state('');

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		failure = '';
		try {
			await api('POST', '/api/reports', { serverId: data.heading.id, target, reason });
			success = true;
			target = '';
			reason = '';
		} catch (err) {
			failure = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>举报玩家 · {data.heading.name}</title></svelte:head>

<section class="mx-auto max-w-xl panel p-5">
	<h1 class="text-xl font-semibold text-white">举报玩家</h1>
	<p class="mt-2 text-sm text-mist-400">举报会保存服务器证据供管理员审核，单靠举报不会封禁玩家。</p>
	{#if !data.signedIn}
		<p class="mt-4 text-sm">
			请先<a class="text-accent underline" href="/sign-in">登录</a>，再到账号页面绑定 Steam。
		</p>
	{:else}
		<form class="mt-5 space-y-4" onsubmit={submit}>
			<label class="block text-sm text-mist-300"
				>玩家名或 SteamID64
				<input class="mt-1 input w-full" bind:value={target} maxlength="200" required />
			</label>
			<label class="block text-sm text-mist-300"
				>原因
				<textarea
					class="mt-1 input w-full"
					bind:value={reason}
					minlength="3"
					maxlength="300"
					rows="3"
					required></textarea>
			</label>
			<button class="btn btn-primary" type="submit" disabled={busy}
				>{busy ? '提交中…' : '提交举报'}</button
			>
		</form>
	{/if}
	{#if success}<p role="status" class="mt-4 text-sm text-green-400">
			已收到举报，系统会保存证据并交由管理员审核。
		</p>{/if}
	{#if failure}<p role="alert" class="mt-4 text-sm text-red-400">{failure}</p>{/if}
</section>
