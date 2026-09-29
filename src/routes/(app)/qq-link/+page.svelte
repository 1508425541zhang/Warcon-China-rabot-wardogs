<script lang="ts">
	let { data, form } = $props();
</script>

<svelte:head><title>QQ 与 Steam 绑定</title></svelte:head>
<main class="mx-auto max-w-xl space-y-6 p-6">
	<h1 class="text-2xl font-bold">QQ 与 Steam 绑定</h1>
	<p>
		先在<a href="/account" class="underline">账号设置</a>中验证 Steam，再在 QQ 群 @机器人发送
		/绑定，将五分钟有效的绑定码填在这里。仅填写你本人触发的绑定码。
	</p>
	<form method="POST" action="?/bind" class="flex gap-3">
		<input
			name="code"
			aria-label="绑定码"
			placeholder="32 位绑定码"
			required
			minlength="32"
			maxlength="32"
			autocomplete="off"
			class="min-w-0 flex-1 rounded border p-2"
		/>
		<button class="rounded border px-4 py-2">绑定</button>
	</form>
	{#if form?.message}<p role="status">{form.message}</p>{/if}
	{#each data.links as link}
		<form
			method="POST"
			action="?/unbind"
			class="flex items-center justify-between gap-4 rounded border p-4"
		>
			<input type="hidden" name="serverId" value={link.server_id} />
			<p>服务器 {link.server_id}<br />Steam {link.steam_id}</p>
			<button class="rounded border px-4 py-2">解绑</button>
		</form>
	{/each}
</main>
