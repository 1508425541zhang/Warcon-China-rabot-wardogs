<script lang="ts">
	// The foot of the Settings tab, org owners only: purge the server's stats. The server's name
	// has to be typed back before the button does anything, and the route checks it again.
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import type { ServerInfo } from '$lib/types';

	let { data }: { data: { server: ServerInfo } } = $props();
	let open = $state(false);
	let typed = $state('');
	let busy = $state(false);
	let done = $state('');
	let named = $derived(typed.trim() === data.server.name);

	async function purge() {
		if (!named || busy) return;
		busy = true;
		try {
			const r = await api<{ counts: { kills: number; matches: number; matchPlayers: number } }>(
				'POST',
				`/api/servers/${encodeURIComponent(data.server.id)}/stats/purge`,
				{ name: typed.trim() }
			);
			done = `${r.counts.kills} kills, ${r.counts.matches} matches and ${r.counts.matchPlayers} match rows deleted.`;
			toast('统计数据已清除。', 'ok');
			open = false;
			typed = '';
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<div class="panel">
	<span class="label-sm">统计</span>
	<p class="mb-3 text-[13px] text-mist-400">
		清除本服务器统计数据：所有已记录的击杀、对局和对局明细将永久删除。玩家会话（在线记录、游玩时间、种子期时间、首次访问）会保留。本服务器的生涯数据和排行榜从下一场对局开始重新计算；当前对局从清除时刻起记录。本操作会写入审计日志。
	</p>
	{#if done}<p class="mb-3 text-[13px] text-ok">{done}</p>{/if}
	{#if !open}
		<button type="button" class="btn btn-danger" onclick={() => (open = true)}>清除统计…</button>
	{:else}
		<div class="flex flex-wrap items-end gap-2">
			<label class="block">
				<span class="label-sm">输入服务器名称以确认</span>
				<input
					class="input w-72 max-w-full"
					type="text"
					bind:value={typed}
					placeholder={data.server.name}
					autocomplete="off"
					spellcheck="false"
				/>
			</label>
			<button type="button" class="btn btn-danger" disabled={!named || busy} onclick={purge}
				>{busy ? 'Purging…' : 'Purge'}</button
			>
			<button
				type="button"
				class="btn"
				disabled={busy}
				onclick={() => {
					open = false;
					typed = '';
				}}>取消</button
			>
		</div>
	{/if}
</div>
