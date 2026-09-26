<script lang="ts">
	import { untrack } from 'svelte';
	import { api, errorMessage } from '$lib/api';
	import { invalidateAll } from '$app/navigation';
	import { fmtTime } from '$lib/format';
	import type { skillBalanceView } from '$lib/server/skill-balance';
	let { data, serverId }: { data: Awaited<ReturnType<typeof skillBalanceView>>; serverId: string } =
		$props();
	let grace = $state(untrack(() => data.rule.graceSeconds)),
		lead = $state(untrack(() => data.rule.leadPoints));
	let busy = $state(false),
		message = $state('');
	const states: Record<string, string> = {
		waiting_safe: '等待安全调队接口',
		cancelled: '候选已作废',
		done: '已完成',
		executing: '执行中',
		partial: '部分执行，请查看明细',
		error: '未完成',
		unknown: '执行中断，结果待核对',
		confirmed: '已确认',
		failed: '失败',
		sending: '已发送／等待确认'
	};
	async function save(enabled: boolean) {
		busy = true;
		message = '';
		try {
			await api('POST', `/api/servers/${serverId}/skill-balance`, {
				enabled,
				graceSeconds: grace,
				leadPoints: lead
			});
			await invalidateAll();
			message = enabled ? '候选监测已启用／保存；自动交换因安全状态缺失而阻止' : '已关闭';
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<section class="space-y-4 panel p-5">
	<h2 class="text-lg font-semibold">
		强弱阵营平衡 · {data.rule.enabled ? '候选监测已开启' : '默认关闭'}
	</h2>
	<p class="text-sm text-mist-300">
		仅三阵营对局：最高分阵营必须比另外两队各高出超过 {lead} 分。取领先方 KPM 最高的3人，与最低分方 KPM
		最低的3人交换；KPM相同时按KD排序。中间阵营不动，每局维护一份候选计划。
	</p>
	<p class="text-sm text-mist-400">
		KPM使用最近180秒有效击杀数÷3（包括枪械、载具等；排除自杀和已知队杀）；KD使用本局已收到的击杀÷max(死亡,1)。只选连续在线满180秒且近期没有面板调队记录的玩家。没有可靠的当前对局Feed或阵营分数时不执行，不能用未知数据补零。两名次榜都没有改善时不调队。
	</p>
	<p class="rounded border border-amber-400/30 p-3 text-sm text-amber-300" role="status">
		自动交换暂不可用：{data.executionBlock}
	</p>
	<p class="text-sm text-mist-400">
		必须在确认死亡或无装备的刚重生阶段才可交换。当前仅监测候选；死亡计数增加或收到Kill
		Feed不能证明现在仍然死亡。
	</p>
	<div class="flex flex-wrap items-center gap-4">
		<label
			>开局／重连保护（秒）<input
				class="ml-2 input w-24"
				type="number"
				min="180"
				max="1800"
				bind:value={grace}
			/></label
		>
		<label
			>领先另外两队的分差（严格大于）<input
				class="ml-2 input w-24"
				type="number"
				min="40"
				max="10000"
				bind:value={lead}
			/></label
		>
		<button class="btn btn-primary" disabled={busy} onclick={() => save(!data.rule.enabled)}
			>{busy
				? '保存中…'
				: data.rule.enabled
					? '关闭候选监测'
					: '开启候选监测（不自动调队）'}</button
		>
		<button class="btn-secondary btn" disabled={busy} onclick={() => save(data.rule.enabled)}
			>保存参数</button
		>
	</div>
	{#if message}<p role="status">{message}</p>{/if}
	<h3 class="font-semibold">最近20局候选计划</h3>
	{#each data.runs as run (run.id)}
		<details class="rounded border border-white/10 p-3">
			<summary>{fmtTime(run.createdAt)} · {states[run.state] ?? run.state} · {run.reason}</summary>
			<p class="my-2 text-sm">领先方：{run.plan.strong}；落后方：{run.plan.weak}</p>
			{#each run.plan.pairs as pair}<p class="text-sm">
					{pair.strong.name}（KPM {pair.strong.kpm.toFixed(2)}／KD {pair.strong.kd.toFixed(2)}） ↔ {pair
						.weak.name}（KPM {pair.weak.kpm.toFixed(2)}／KD {pair.weak.kd.toFixed(2)}）
				</p>{/each}
			{#each run.moves as move}<p class="mt-1 text-xs">
					<a class="text-accent" href="/server/{serverId}/players/{move.steamId}">{move.steamId}</a
					>：{move.from} → {move.to} · {states[move.state] ?? move.state}
					{move.reason ?? ''}
				</p>{/each}
		</details>
	{:else}<p class="text-sm text-mist-400">
			暂无候选计划。开启后会自动选人；只有接入可靠的死亡／重生条件调队接口后，才能实现自动交换。
		</p>{/each}
</section>
