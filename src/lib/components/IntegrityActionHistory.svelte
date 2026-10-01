<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { integrityDeliveryReason } from '$lib/integrity-delivery-display';
	import { HISTORY_PAGE_SIZES, type HistoryRetentionPolicy } from '$lib/integrity-retention';
	import type { integrityActionPage } from '$lib/server/integrity/action-history';
	type History = Awaited<ReturnType<typeof integrityActionPage>>;
	let {
		serverId,
		actions,
		pagination,
		policy,
		revision,
		lastCleanup,
		canConfigure,
		lang,
		stateLabel
	}: {
		serverId: string;
		actions: History['rows'];
		pagination: Omit<History, 'rows'>;
		policy: HistoryRetentionPolicy;
		revision: string;
		lastCleanup: { at: string; removed: number } | null;
		canConfigure: boolean;
		lang: 'zh' | 'en';
		stateLabel: (item: History['rows'][number]) => string;
	} = $props();
	let settingsOpen = $state(false);
	// svelte-ignore state_referenced_locally -- refreshed by the effect while settings are closed.
	let draft = $state<HistoryRetentionPolicy>({ ...policy });
	let saving = $state(false);
	let error = $state('');
	$effect(() => {
		if (!settingsOpen) draft = { ...policy };
	});
	const when = (value: string) => new Date(value).toLocaleString(lang === 'zh' ? 'zh-CN' : 'en-US');
	function pageLink(page: number, filter = pagination.filter) {
		const query = new URLSearchParams({
			actionsPage: String(page),
			actionsFilter: filter,
			actionsBefore: pagination.before
		});
		return `/server/${serverId}/integrity?${query}#punishment-history`;
	}
	async function save(event: SubmitEvent) {
		event.preventDefault();
		saving = true;
		error = '';
		try {
			const response = await fetch(`/api/server/${serverId}/integrity/history-retention`, {
				method: 'PUT',
				headers: { 'content-type': 'application/json', 'x-requested-with': 'warcon' },
				body: JSON.stringify({ policy: draft, revision })
			});
			const result = await response.json();
			if (!response.ok) throw Error(result.error?.message ?? '保存失败');
			settingsOpen = false;
			await invalidateAll();
		} catch (cause) {
			error = cause instanceof Error ? cause.message : '保存失败';
		} finally {
			saving = false;
		}
	}
</script>

<section id="punishment-history" class="mb-6 panel p-4">
	<h3 class="mb-3 text-base font-semibold text-white">
		{lang === 'zh' ? '处罚记录（自动／人工）' : 'Actions (automatic / human review)'}
	</h3>
	<div class="mb-3 flex flex-wrap items-center gap-3 text-sm">
		{#each [['all', '全部'], ['automatic', '自动'], ['manual', '人工']] as [filter, label]}
			<a
				href={pageLink(1, filter as History['filter'])}
				aria-current={pagination.filter === filter ? 'page' : undefined}
				class:font-bold={pagination.filter === filter}>{label}</a
			>
		{/each}
		<span class="text-mist-400">共 {pagination.total} 条 · 每页 {pagination.pageSize} 条</span>
		<a href={`/server/${serverId}/integrity?actionsFilter=${pagination.filter}#punishment-history`}
			>最新记录</a
		>
	</div>
	<p class="mb-3 text-xs text-mist-400">
		{policy.autoDeleteEnabled
			? `自动清理：每台服务器保留最新 ${policy.maxRecords} 条，历史最长 ${policy.maxAgeDays} 天。`
			: '自动清理未开启。'} 近 7 天、生效中、未完成或结果不确定的处罚受保护，不因超额删除。
	</p>
	{#if lastCleanup}<p class="mb-3 text-xs text-mist-400">
			上次清理：{when(lastCleanup.at)} · 已删除 {lastCleanup.removed} 条记录及相关数据
		</p>{/if}
	{#if canConfigure}
		<details bind:open={settingsOpen} class="mb-4 text-sm">
			<summary class="cursor-pointer">分页与自动保留设置</summary>
			<form onsubmit={save} class="mt-3 flex flex-wrap items-end gap-4">
				<label
					>每页条数<select bind:value={draft.pageSize}
						>{#each HISTORY_PAGE_SIZES as size}<option value={size}>{size}</option>{/each}</select
					></label
				>
				<label
					>自动＋人工保留上限<input
						type="number"
						min="100"
						max="100000"
						required
						bind:value={draft.maxRecords}
					/></label
				>
				<label
					>历史最长天数<input
						type="number"
						min="7"
						max="3650"
						required
						bind:value={draft.maxAgeDays}
					/></label
				>
				<label><input type="checkbox" bind:checked={draft.autoDeleteEnabled} />自动清理</label>
				<button class="btn" disabled={saving} type="submit">{saving ? '保存中…' : '保存'}</button>
			</form>
			<p class="mt-2 text-xs text-mist-400">
				每小时分批清理旧处罚及相关已完成任务、通知、证据和普通模型结果。保护记录可能使数量暂时超过上限。
			</p>
			{#if error}<p role="alert" class="mt-2 text-warn">{error}</p>{/if}
		</details>
	{/if}
	{#if actions.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>时间</th><th>玩家</th><th>来源／档位</th><th>处置指令（不代表已执行）</th><th
							>执行状态</th
						><th>执行说明</th><th>生效时间</th><th>到期</th></tr
					></thead
				>
				<tbody
					>{#each actions as item (item.id)}<tr>
							<td>{when(item.createdAt)}</td><td class="font-mono">{item.steamId}</td>
							<td class="whitespace-normal"
								>{item.source === 'SHORT_MODEL'
									? '短窗模型'
									: item.source === 'LONG_MODEL'
										? '长时序模型'
										: item.source === 'REVIEW'
											? '人工操作'
											: '旧规则／委员会'}
								{item.percentileLabel}
								{#if item.score !== null}<div class="text-xs text-mist-400">
										分数 {item.score.toFixed(5)}{#if item.threshold !== null}
											· 阈值 {item.threshold.toFixed(5)}{/if}
									</div>{/if}</td
							>
							<td
								>{item.source === 'REVIEW'
									? '人工确认违规（7天；保留更长期封禁）'
									: item.action === 'KICK'
										? '踢出指令'
										: item.action === 'WARNING'
											? '警惕'
											: item.action}</td
							>
							<td>{stateLabel(item)}</td><td class="max-w-md whitespace-normal"
								>{integrityDeliveryReason(item.deliveryReason, lang)}</td
							>
							<td>{item.effectiveAt ? when(item.effectiveAt) : '—'}</td><td
								>{item.expiresAt ? when(item.expiresAt) : '—'}</td
							>
						</tr>{/each}</tbody
				>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">暂无处罚记录。</p>{/if}
	<nav aria-label="处罚记录分页" class="mt-3 flex items-center gap-4 text-sm">
		{#if pagination.page > 1}<a href={pageLink(pagination.page - 1)}>上一页</a>{/if}
		<span>第 {pagination.page}／{pagination.pages} 页</span>
		{#if pagination.page < pagination.pages}<a href={pageLink(pagination.page + 1)}>下一页</a>{/if}
	</nav>
</section>
