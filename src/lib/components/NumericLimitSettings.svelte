<script lang="ts">
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import type { numericLimitView } from '$lib/server/numeric-limits';
	let { data, serverId }: { data: Awaited<ReturnType<typeof numericLimitView>>; serverId: string } =
		$props();
	let config = $state(untrack(() => ({ ...data.config })));
	let kpmOn = $state(untrack(() => data.config.kpm !== null)),
		kdOn = $state(untrack(() => data.config.kd !== null)),
		cashOn = $state(untrack(() => data.config.cash !== null));
	let busy = $state(false),
		message = $state('');
	async function save(enabled: boolean) {
		busy = true;
		message = '';
		try {
			if (
				enabled &&
				((kpmOn && !(Number(config.kpm) > 0)) ||
					(kdOn && !(Number(config.kd) > 0)) ||
					(cashOn && !(Number(config.cash) > 0)))
			)
				throw new Error('请为勾选项目填写大于0的上限。');
			await api('POST', `/api/servers/${serverId}/numeric-limits`, {
				...config,
				enabled,
				kpm: kpmOn ? config.kpm : null,
				kd: kdOn ? config.kd : null,
				cash: cashOn ? config.cash : null
			});
			await invalidateAll();
			message = enabled ? '已保存并启用；等待完整统计窗口' : '已关闭';
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	const states: Record<string, string> = {
		delivered: '已送达',
		sending: '发送中／待核对',
		unknown: '结果未知，请人工核对',
		skipped: '规则变化，已跳过'
	};
</script>

<section class="space-y-4 panel p-5">
	<h2 class="text-lg font-semibold">
		硬性数值限制 · {data.config.enabled ? '已启用' : '默认关闭'}
	</h2>
	<p class="text-sm text-mist-300">
		供社区服自定入服规则使用，不表示玩家作弊。每项单独勾选并填写上限；严格超过才触发。第一次私聊警告，警告后再经过一个完整新窗口仍超限才踢出，不封禁。每名玩家每局最多踢出一次。
	</p>
	<div class="grid gap-4 md:grid-cols-3">
		<label
			><input type="checkbox" bind:checked={kpmOn} /> 限制 KPM<input
				aria-label="KPM 上限"
				class="mt-2 input w-full"
				type="number"
				min="0.01"
				max="100000000"
				step="any"
				disabled={!kpmOn}
				bind:value={config.kpm}
			/></label
		>
		<label
			><input type="checkbox" bind:checked={kdOn} /> 限制 KD<input
				aria-label="KD 上限"
				class="mt-2 input w-full"
				type="number"
				min="0.01"
				max="100000000"
				step="any"
				disabled={!kdOn}
				bind:value={config.kd}
			/></label
		>
		<label
			><input type="checkbox" bind:checked={cashOn} /> 限制金钱净增长／分钟<input
				aria-label="每分钟金钱净增长上限"
				class="mt-2 input w-full"
				type="number"
				min="0.01"
				max="100000000"
				step="any"
				disabled={!cashOn}
				bind:value={config.cash}
			/></label
		>
	</div>
	<div class="flex flex-wrap gap-4">
		<label
			>统计／再次判定窗口（秒）<input
				class="ml-2 input w-24"
				type="number"
				min="60"
				max="900"
				step="1"
				bind:value={config.windowSeconds}
			/></label
		>
		<label
			>KD 最低击杀数<input
				class="ml-2 input w-24"
				type="number"
				min="1"
				max="1000"
				step="1"
				bind:value={config.minKills}
			/></label
		>
	</div>
	<p class="text-sm text-mist-400">
		KPM＝窗口内计分板击杀增量÷分钟数，包含所有计分板计入的击杀。KD＝本局击杀÷max(死亡,1)，警告后须新增击杀才再次判定
		KD。金钱为窗口余额净变化÷分钟数，受消费影响，不是总收入。缺失、重连、计数重置或采样间隔超过65秒时等待重新积累。保存设置后重新开始积累和警告；原有风控级别不变。
	</p>
	<div class="flex gap-3">
		<button class="btn" disabled={busy} onclick={() => save(true)}
			>{data.config.enabled ? '保存设置' : '保存并启用'}</button
		><button class="btn" disabled={busy || !data.config.enabled} onclick={() => save(false)}
			>关闭</button
		>
	</div>
	{#if message}<p role="status">{message}</p>{/if}
	<h3 class="font-semibold">最近50条执行记录</h3>
	{#each data.events as event}
		<div class="border-t border-white/10 py-2 text-sm">
			<span
				>{fmtTime(event.createdAt)} · {event.steamId} · {event.action === 'warn' ? '警告' : '踢出'} ·
				{states[event.state] ?? event.state}</span
			>
			<pre class="overflow-x-auto text-xs whitespace-pre-wrap">{JSON.stringify(
					event.evidence,
					null,
					2
				)}</pre>
		</div>
	{:else}<p class="text-sm text-mist-400">暂无记录。未启用或数据不足时不会处罚。</p>{/each}
</section>
