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
	<h1 class="text-xl font-semibold text-white">举报玩家 / Report a player</h1>
	<p class="mt-2 text-sm text-mist-400">
		举报会保存服务器证据供管理员审核，单靠举报不会封禁玩家。Reports preserve server evidence for
		review; reports alone do not cause bans.
	</p>
	{#if !data.signedIn}
		<p class="mt-4 text-sm">
			<a class="text-accent underline" href="/sign-in">登录 / Sign in</a> 后请在账号页面绑定 Steam。 /
			Link Steam on your account page after signing in.
		</p>
	{:else}
		<form class="mt-5 space-y-4" onsubmit={submit}>
			<label class="block text-sm text-mist-300"
				>玩家名或 SteamID64 / Player name or SteamID64
				<input class="mt-1 input w-full" bind:value={target} maxlength="200" required />
			</label>
			<label class="block text-sm text-mist-300"
				>原因 / Reason
				<textarea
					class="mt-1 input w-full"
					bind:value={reason}
					minlength="3"
					maxlength="300"
					rows="3"
					required></textarea>
			</label>
			<button class="btn btn-primary" type="submit" disabled={busy}
				>{busy ? '提交中… / Sending…' : '提交举报 / Send report'}</button
			>
		</form>
	{/if}
	{#if success}<p role="status" class="mt-4 text-sm text-green-400">
			已收到举报，系统会保存证据并交由管理员审核。 / Report received for review.
		</p>{/if}
	{#if failure}<p role="alert" class="mt-4 text-sm text-red-400">{failure}</p>{/if}
</section>
