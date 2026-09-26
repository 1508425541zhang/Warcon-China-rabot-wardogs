<script lang="ts">
	import { api, errorMessage } from '$lib/api';
	import { invalidateAll } from '$app/navigation';
	import { fmtTime } from '$lib/format';
	import { onMount, untrack } from 'svelte';
	import { refreshVisible } from '$lib/refresh-visible';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	let enabled = $state(untrack(() => data.rule.enabled)),
		grace = $state(untrack(() => data.rule.graceSeconds));
	let capacities = $state<Record<string, string>>(
		Object.fromEntries(
			Object.entries(untrack(() => data.rule.capacities) as Record<string, number>).map(
				([k, v]) => [k, String(v)]
			)
		)
	);
	let busy = $state(false),
		message = $state('');
	const names: Record<string, string> = {
		pending: '等待复核',
		executing: '执行中',
		restored: '已调回',
		warned: '已警告',
		kicked: '已踢出',
		skipped: '已豁免／放行',
		error: '执行失败'
	};
	const reasons: Record<string, string> = {
		INITIAL_OR_UNASSIGNED: '首次选边或空白阵营，不处理',
		AUTHORIZED_MOVE: 'Warcon 调队／平衡操作，不处理',
		ROUND_GRACE: '开局、换图或重连保护',
		RESTORE: '检测到自行换边，等待复核'
	};
	onMount(() => refreshVisible(invalidateAll));
	async function save() {
		busy = true;
		message = '';
		try {
			await api('POST', `/api/servers/${data.server.id}/faction-lock`, {
				enabled,
				graceSeconds: grace,
				capacities: Object.fromEntries(
					Object.entries(capacities)
						.filter(([, v]) => v.trim())
						.map(([k, v]) => [k, Number(v)])
				)
			});
			await invalidateAll();
			message = '已保存';
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>禁止自行换边</title></svelte:head>
<section class="panel p-5">
	<h1 class="text-xl font-semibold">禁止自行换边</h1>
	<p class="mt-2 text-sm text-mist-400">
		依次排除 Warcon 管理员调队、Warcon
		自动平衡、开局／换图保护、空白阵营首次选边。其他换边先调回原阵营；原阵营已满且严格领先其他阵营时，发送警告，至少
		8 秒后重新检查再踢出。原阵营已满但不领先时保持现状。
	</p>
	<p class="mt-2 text-sm text-mist-400">
		管理员与平衡必须经 Warcon
		调队接口执行，命令发出前会自动记录豁免。若改用游戏内命令或其他面板，请先关闭本功能。轮询只能观察两次采样之间的阵营结果，不能识别期间发生的所有变化。
	</p>
	<div class="mt-4 space-y-4">
		<label class="flex gap-2"
			><input type="checkbox" bind:checked={enabled} />启用禁止自行换边</label
		>
		<label class="flex items-center gap-3"
			>开局／换图保护（秒）<input
				type="number"
				min="30"
				max="600"
				bind:value={grace}
				class="w-24 rounded border border-white/20 bg-transparent p-2"
			/></label
		>
		<fieldset class="rounded border border-white/10 p-3">
			<legend>原阵营容量（可选）</legend>
			<p class="text-xs text-mist-400">
				填写经过确认的实际容量。留空时不会按服务器总人数推算，而是尝试调回；仅服务器明确返回阵营已满才进入警告检查。普通失败、网络错误不会触发踢出。
			</p>
			{#each [...new Set([...data.teams, ...Object.keys(capacities)])] as team}<label
					class="mt-2 flex items-center gap-3"
					>{team}<input
						type="number"
						min="1"
						max="1000"
						value={capacities[team] ?? ''}
						oninput={(e) => (capacities[team] = e.currentTarget.value)}
						placeholder="未知"
						class="w-24 rounded border border-white/20 bg-transparent p-2"
					/></label
				>{/each}
		</fieldset>
		<button class="btn-primary" onclick={save} disabled={busy}
			>{busy ? '保存中…' : '保存设置'}</button
		><span class="ml-3 text-sm">{message}</span>
	</div>
	<a class="mt-4 inline-block text-accent" href="/server/{data.server.id}/automation"
		>← 返回自动化</a
	>
</section>
<section class="mt-4 panel p-4">
	<h2 class="font-semibold">最近 50 条换边记录</h2>
	<div class="mt-3 table-wrap">
		<table>
			<thead><tr><th>时间</th><th>玩家</th><th>阵营变化</th><th>状态</th><th>原因</th></tr></thead
			><tbody
				>{#each data.events as e (e.id)}<tr
						><td>{fmtTime(e.createdAt)}</td><td
							><a class="text-accent" href="/server/{data.server.id}/players/{e.steamId}"
								>{e.steamId}</a
							></td
						><td>{e.fromFaction || '空白'} → {e.toFaction || '空白'}</td><td
							>{names[e.state] ?? e.state}</td
						><td>{reasons[e.reason] ?? e.reason}</td></tr
					>{:else}<tr><td colspan="5">尚无记录；启用后会记录换边与处理结果。</td></tr>{/each}</tbody
			>
		</table>
	</div>
</section>
