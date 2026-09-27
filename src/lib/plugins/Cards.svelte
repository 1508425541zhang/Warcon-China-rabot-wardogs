<script lang="ts">
	import { columnLabels, type PluginComponentProps } from './sdk';
	let { plugin, snapshot }: PluginComponentProps = $props();
	const format = (v: number | null | undefined) => (v == null ? '—' : v.toLocaleString('zh-CN'));
</script>

<div
	class="plugin-grid"
	style:--plugin-columns={plugin.style.columns}
	style:--plugin-accent={plugin.style.accent}
	class:compact={plugin.style.density === 'compact'}
>
	{#each plugin.widgets as widget, index (index)}
		<article class="plugin-card panel">
			<h2 class="font-semibold text-white">{widget.title}</h2>
			{#if widget.type === 'metric'}
				<p class="metric mt-3 font-mono text-3xl">{format(snapshot?.metrics[widget.metric])}</p>
			{:else if widget.type === 'text'}
				<p class="mt-3 text-sm whitespace-pre-wrap text-mist-300">{widget.text}</p>
			{:else}
				<div class="mt-3 table-wrap">
					<table>
						<thead
							><tr
								>{#each widget.columns as column}<th>{columnLabels[column]}</th>{/each}</tr
							></thead
						>
						<tbody
							>{#each [...(snapshot?.players ?? [])]
								.sort((a, b) => (b[widget.sortBy] ?? -1) - (a[widget.sortBy] ?? -1))
								.slice(0, widget.limit) as player, row (row)}
								<tr
									>{#each widget.columns as column}<td
											>{typeof player[column] === 'number'
												? format(player[column] as number)
												: (player[column] ?? '—')}</td
										>{/each}</tr
								>
							{:else}<tr><td colspan={widget.columns.length}>暂无玩家数据</td></tr>{/each}</tbody
						>
					</table>
				</div>
			{/if}
		</article>
	{/each}
</div>

<style>
	.plugin-grid {
		display: grid;
		grid-template-columns: repeat(var(--plugin-columns), minmax(0, 1fr));
		gap: 1rem;
	}
	.plugin-card {
		padding: 1.25rem;
		min-width: 0;
		border-top: 2px solid var(--plugin-accent);
	}
	.metric {
		color: var(--plugin-accent);
	}
	.compact .plugin-card {
		padding: 0.75rem;
	}
	@media (max-width: 760px) {
		.plugin-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
