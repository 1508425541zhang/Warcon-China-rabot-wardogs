<script lang="ts">
	import type { PluginComponentProps } from '$lib/plugins/sdk';
	let { plugin, snapshot }: PluginComponentProps = $props();
	let leaders = $derived(
		[...(snapshot?.players ?? [])].sort((a, b) => (b.kills ?? -1) - (a.kills ?? -1)).slice(0, 3)
	);
</script>

<section class="round-summary panel" style:--plugin-accent={plugin.style.accent}>
	<h2 class="text-xl font-semibold text-white">{snapshot?.server.name ?? '请先关联服务器'}</h2>
	<p class="mt-2 text-sm text-mist-400">
		地图：{snapshot?.server.map ?? '—'} · 在线：{snapshot?.metrics.online ?? '—'} · 平均延迟：{snapshot
			?.metrics.averagePing ?? '—'} ms
	</p>
	<h3 class="mt-5 font-semibold text-white">在线玩家击杀前三</h3>
	<ol class="mt-3 space-y-2">
		{#each leaders as player, i (i)}<li class="flex justify-between gap-4">
				<span>{i + 1}. {player.name}</span><strong>{player.kills ?? '—'} 击杀</strong>
			</li>{:else}<li class="text-mist-400">暂无玩家数据</li>{/each}
	</ol>
	<p class="mt-5 text-xs text-mist-400">本例仅展示当前在线玩家快照，离线玩家不计入。</p>
</section>

<style>
	/* Svelte scopes these styles to this component. No :global or document selectors. */
	.round-summary {
		border-top: 3px solid var(--plugin-accent);
		padding: 1.5rem;
	}
	strong {
		color: var(--plugin-accent);
	}
</style>
