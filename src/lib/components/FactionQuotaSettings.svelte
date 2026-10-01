<script lang="ts">
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { quotaColors, quotaLabels, quotaTeams, type QuotaColor } from '$lib/faction-quota-policy';
	import type { FactionScore } from '$lib/types';
	import type { factionQuotaView } from '$lib/server/faction-quota';
	let {
		data,
		serverId,
		scores
	}: {
		data: Awaited<ReturnType<typeof factionQuotaView>>;
		serverId: string;
		scores: FactionScore[];
	} = $props();
	let limits = $state(untrack(() => ({ ...data.config.limits }))),
		grace = $state(untrack(() => data.config.graceSeconds));
	let busy = $state(false),
		message = $state('');
	const teams = $derived(quotaTeams(scores));
	function preset(closed: QuotaColor) {
		for (const color of quotaColors) limits[color] = color === closed ? 0 : 50;
	}
	async function save(enabled: boolean) {
		busy = true;
		message = '';
		try {
			await api('POST', `/api/servers/${serverId}/faction-quota`, {
				revision: data.config.revision,
				enabled,
				limits,
				graceSeconds: grace
			});
			await invalidateAll();
			message = enabled
				? '已保存启用。修改原生加入限制时，将等待下一局生效后调队。'
				: '已关闭／保存；此前由本功能修改的原生加入限制将恢复，下一局生效。';
		} catch (error) {
			message = errorMessage(error);
			await invalidateAll();
		} finally {
			busy = false;
		}
	}
</script>

<section class="space-y-4" aria-label="50V50 阵营人数配额">
	<h2 class="text-lg font-semibold">50V50 模式 · 蓝／红／绿人数配额</h2>
	<p class="text-sm text-mist-300">
		每方可设 0～100 人，合计最多 100 人，至少开放两方。0 表示该方不留玩家。例如蓝 50、红 50、绿
		0：满员时为 50V50；人数不足时按配额比例分配，奇数人数允许相差 1 人。
	</p>
	<div class="flex flex-wrap gap-2">
		{#each quotaColors as color}<button
				type="button"
				class="btn"
				disabled={busy}
				onclick={() => preset(color)}>{quotaLabels[color]}关闭 · 另外两方各50</button
			>{/each}
	</div>
	<div class="grid gap-3 sm:grid-cols-3">
		{#each quotaColors as color}<label class="text-sm"
				>{quotaLabels[color]} · {teams?.[color] ?? '等待识别'}<input
					class="mt-1 input"
					type="number"
					min="0"
					max="100"
					step="1"
					bind:value={limits[color]}
					disabled={busy}
				/></label
			>{/each}
	</div>
	<label class="block text-sm"
		>开局／重连保护（秒）<input
			class="mt-1 input"
			type="number"
			min="30"
			max="600"
			step="1"
			bind:value={grace}
			disabled={busy}
		/></label
	>
	<p class="text-sm text-mist-400">
		启用后通过官方管理员接口逐人调队并重生，以新玩家快照确认结果。游戏仍保留三个阵营入口，进入关闭方的玩家会被转到开放方；这不是原生隐藏第三阵营的游戏模式。没有空位时暂停，不会踢人腾位。
	</p>
	<p class="text-sm text-mist-400">
		启用时解除游戏的三方人数差加入限制，该设置下一局生效；关闭时恢复本功能修改前的值。请先关闭“禁止自行换边”和“强弱阵营平衡”。达到各50人还需要游戏服务器实际容量支持100人。
	</p>
	<div class="flex flex-wrap gap-2">
		<button class="btn btn-primary" disabled={busy} onclick={() => save(true)}
			>启用并保存配额</button
		><button class="btn" disabled={busy} onclick={() => save(false)}
			>{data.config.enabled ? '关闭并保存' : '仅保存，保持关闭'}</button
		><button class="btn" disabled={busy} onclick={() => invalidateAll()}>刷新状态</button>
	</div>
	{#if message}<p role="status" class="text-sm">{message}</p>{/if}
	<p class="text-sm">
		当前状态：{data.config.preparing
			? '正在准备配置；暂不调队'
			: data.config.enabled
				? data.runtime.reason
				: '未启用'}
	</p>
	{#if data.runtime.counts}<p class="text-sm">
			最近人数：蓝 {data.runtime.counts.blue} / 红 {data.runtime.counts.red} / 绿 {data.runtime
				.counts.green}
		</p>{/if}
	<div class="overflow-auto">
		<table class="w-full text-left text-sm">
			<thead><tr><th>玩家</th><th>调队</th><th>结果</th><th>时间</th></tr></thead><tbody
				>{#each data.runtime.events as event}<tr
						><td>{event.steamId}</td><td>{event.from} → {event.to}</td><td
							>{event.state === 'confirmed'
								? '已确认'
								: event.state === 'sending'
									? '待快照确认'
									: event.state === 'cancelled'
										? '已取消'
										: '结果待核对'}<br />{event.reason}</td
						><td>{new Date(event.at).toLocaleString()}</td></tr
					>{:else}<tr><td colspan="4">尚无配额调队记录。</td></tr>{/each}</tbody
			>
		</table>
	</div>
</section>
