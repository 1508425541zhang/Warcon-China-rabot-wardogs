<script lang="ts">
	import type { loadShortRisk } from '$lib/server/integrity/short-risk';
	let { view, serverId }: { view: Awaited<ReturnType<typeof loadShortRisk>>; serverId: string } =
		$props();
	let dialog: HTMLDialogElement;
	let dismissed = new Set<string>();
	let alerts = $derived(view.players.filter((p) => p.level === 'warning' || p.level === 'kick'));
	$effect(() => {
		const fresh = alerts.filter(
			(p) => !dismissed.has(`${p.player_id}:${p.scope?.join(':')}:${p.level}`)
		);
		if (fresh.length && dialog && !dialog.open) {
			for (const p of fresh) dismissed.add(`${p.player_id}:${p.scope?.join(':')}:${p.level}`);
			dialog.showModal();
		}
	});
	const states: Record<string, string> = {
		warning: '警惕',
		pending: '执行中',
		delivered: '踢出请求已接受',
		unknown: '执行结果未知，未重发'
	};
</script>

<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">无监督短窗 · 玩家异常观察</h3>
	<p class="text-dim mt-2 text-sm">
		60／120 秒窗口，每 10 秒更新。P99.6 弹出警惕；连续五窗均超过 P99.9，再按由旧到新 1:2:3:4:5
		加权确认后踢出。百分位表示异常排名，不是作弊概率。
	</p>
	<p class="mt-2 text-sm">
		自动踢出：{view.autoKick ? '已开启' : '未开启'} · 最近更新：{view.evaluatedAt
			? new Date(view.evaluatedAt).toLocaleString('zh-CN')
			: '—'}
	</p>
	{#if !view.available}<p class="mt-3 text-warn" role="status">
			短窗服务暂无新鲜结果；不把缺失数据当作正常，也不执行处罚。
		</p>{/if}
	{#if alerts.length}<p
			class="mt-3 rounded-ctl border border-warn/30 bg-warn/10 p-3 text-warn"
			role="alert"
		>
			警惕：{alerts.length} 名玩家达到异常尾部，请查看下方名单。
		</p>{/if}
	<div class="mt-3 overflow-x-auto">
		<table class="w-full text-left text-sm">
			<thead><tr><th>玩家</th><th>异常百分位</th><th>原始分数</th><th>状态</th></tr></thead>
			<tbody
				>{#each alerts as p}<tr class="border-line border-t"
						><td class="py-2"
							><a class="underline" href="/server/{serverId}/players/{p.player_id}"
								>{p.name ?? p.player_id}</a
							>
							<div class="text-dim text-xs">{p.player_id}</div></td
						><td>{p.percentile?.toFixed(3) ?? '—'}%</td><td>{p.anomaly_score?.toFixed(5) ?? '—'}</td
						><td class:text-warn={p.level !== 'normal'}
							>{p.level === 'kick'
								? 'P99.9 踢出候选'
								: p.level === 'warning'
									? 'P99.6 警惕'
									: '未达警惕阈值'}</td
						></tr
					>{/each}</tbody
			>
		</table>
		{#if !alerts.length}<p class="text-dim py-3">当前没有达到警惕档位的玩家。</p>{/if}
	</div>
	<p class="text-dim mt-3 text-xs">警惕与踢出记录已合并到下方处罚记录。</p>
</section>
<dialog
	bind:this={dialog}
	class="max-w-xl rounded-xl border border-warn/40 bg-slate-950 p-6 text-white backdrop:bg-black/70"
	aria-labelledby="short-risk-alert-title"
>
	<h2 id="short-risk-alert-title" class="text-xl font-semibold text-warn">
		警惕：发现异常尾部玩家
	</h2>
	<p class="text-dim mt-2 text-sm">
		正常窗口、数据中断或换局会重新累计。警惕与五窗确认后的实际处罚统一显示在处罚记录中。
	</p>
	<ul class="mt-4 space-y-2">
		{#each alerts as p}<li>
				{p.name ?? p.player_id} · {p.percentile?.toFixed(3)}% · {p.level === 'kick'
					? 'P99.9'
					: 'P99.6 警惕'}
			</li>{/each}
	</ul>
	<button class="border-line mt-5 rounded border px-4 py-2" onclick={() => dialog.close()}
		>知道了，查看名单</button
	>
</dialog>
