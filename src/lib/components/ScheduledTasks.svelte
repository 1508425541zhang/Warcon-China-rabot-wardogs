<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { scheduleActionLabel, type ScheduledAction } from '$lib/scheduled-task-policy';
	import type { scheduledTasksView } from '$lib/server/scheduled-tasks';
	import type { TriggerView } from '$lib/types';
	let {
		serverId,
		data,
		triggers,
		catalogue,
		causes,
		groups
	}: {
		serverId: string;
		data: Awaited<ReturnType<typeof scheduledTasksView>>;
		triggers: TriggerView[];
		catalogue: { cause: string; label: string }[];
		causes: string[];
		groups: string[];
	} = $props();
	let name = $state(''),
		when = $state<'at' | 'next_match'>('next_match'),
		at = $state('');
	let kind = $state<ScheduledAction['kind']>('weapon_restriction'),
		actionEnabled = $state(true);
	let selected = $state<string[]>(untrack(() => [...causes]));
	let selectedGroups = $state<string[]>(untrack(() => [...groups]));
	let triggerId = $state(''),
		message = $state(''),
		custom = $state(''),
		search = $state('');
	let busy = $state(false),
		error = $state(''),
		page = $state(1);
	let view = $state(untrack(() => data));
	$effect(() => {
		view = data;
	});
	onMount(() => {
		let disposed = false,
			fetching = false;
		const timer = setInterval(async () => {
			if (
				busy ||
				fetching ||
				document.visibilityState !== 'visible' ||
				!view.enabled ||
				!view.tasks.some((t) => ['pending', 'queued'].includes(t.state))
			)
				return;
			const revision = view.revision,
				currentServer = serverId;
			fetching = true;
			try {
				const latest = await api<typeof view>(
					'GET',
					`/api/servers/${currentServer}/scheduled-tasks`
				);
				if (!disposed && !busy && currentServer === serverId && view.revision === revision)
					view = latest;
			} catch {
				/* Mutations and explicit refresh expose errors; a background read is optional. */
			} finally {
				fetching = false;
			}
		}, 15000);
		return () => {
			disposed = true;
			clearInterval(timer);
		};
	});
	let sorted = $derived([...view.tasks].sort((a, b) => b.createdAt.localeCompare(a.createdAt)));
	let pages = $derived(Math.max(1, Math.ceil(sorted.length / 20)));
	let shown = $derived(sorted.slice((Math.min(page, pages) - 1) * 20, Math.min(page, pages) * 20));
	let choices = $derived(
		catalogue
			.filter((c) => (c.cause + ' ' + c.label).toLowerCase().includes(search.toLowerCase()))
			.slice(0, 100)
	);
	const states: Record<string, string> = {
		pending: '等待执行',
		queued: '等待广播确认',
		applied: '已执行',
		cancelled: '已取消',
		skipped: '已跳过',
		delivered: '已发送',
		sending: '发送中',
		failed: '发送失败',
		unknown: '发送结果未知'
	};
	async function change(body: Record<string, unknown>) {
		busy = true;
		error = '';
		try {
			await api('POST', `/api/servers/${serverId}/scheduled-tasks`, {
				revision: view.revision,
				...body
			});
			await invalidateAll();
		} catch (e) {
			error = errorMessage(e);
			await invalidateAll();
		} finally {
			busy = false;
		}
	}
	async function create(event: SubmitEvent) {
		event.preventDefault();
		let action: ScheduledAction;
		if (kind === 'weapon_restriction')
			action = {
				kind,
				enabled: actionEnabled,
				causes: [
					...new Set([
						...selected,
						...custom
							.split(/\r?\n/)
							.map((s) => s.trim())
							.filter(Boolean)
					])
				],
				groups: selectedGroups as ('items' | 'vehicles' | 'buildables')[]
			};
		else if (kind === 'faction_lock') action = { kind, enabled: actionEnabled };
		else if (kind === 'trigger') action = { kind, enabled: actionEnabled, triggerId };
		else action = { kind, message };
		if (when === 'at' && (!at || !Number.isFinite(Date.parse(at + '+08:00')))) {
			error = '请选择执行时间。';
			return;
		}
		await change({
			operation: 'create',
			task: {
				name,
				when:
					when === 'at'
						? { kind: 'at', at: new Date(at + '+08:00').toISOString() }
						: { kind: 'next_match' },
				action
			}
		});
	}
</script>

<section class="space-y-4">
	<p class="text-sm text-mist-400">
		任务执行一次。可指定时间（北京时间
		UTC+8），或等待当前对局结束后的新一局。服务器正常观测时生效；错过超过5分钟会跳过。暂停插件后不执行任务。
	</p>
	<label class="flex items-center gap-2"
		><input
			type="checkbox"
			checked={view.enabled}
			disabled={busy}
			onchange={(event) => change({ operation: 'settings', enabled: event.currentTarget.checked })}
		/>启用计划任务插件</label
	>
	{#if !view.enabled}<p class="text-warn">插件已暂停；可以先创建任务，再启用。</p>{/if}
	<form onsubmit={create} class="space-y-3 rounded border border-white/10 p-4">
		<label class="block"
			>任务名称<input
				class="mt-1 w-full"
				required
				maxlength="80"
				bind:value={name}
				placeholder="下一局禁止狙击枪"
			/></label
		>
		<div class="grid gap-3 sm:grid-cols-2">
			<label
				>执行时机<select class="mt-1 w-full" bind:value={when}
					><option value="next_match">当前对局结束后的新一局</option><option value="at"
						>指定日期与时间</option
					></select
				></label
			>
			{#if when === 'at'}<label
					>执行时间（UTC+8）<input
						class="mt-1 w-full"
						type="datetime-local"
						required
						bind:value={at}
					/></label
				>{/if}
			<label
				>执行项目<select class="mt-1 w-full" bind:value={kind}
					><option value="weapon_restriction">武器／载具限制</option><option value="faction_lock"
						>禁止玩家自行换阵营</option
					><option value="trigger">启停其他自动化规则</option><option value="broadcast"
						>服务器广播</option
					></select
				></label
			>
			{#if kind !== 'broadcast'}<label
					>操作<select class="mt-1 w-full" bind:value={actionEnabled}
						><option value={true}>{kind === 'faction_lock' ? '禁止自行换阵营' : '启用'}</option
						><option value={false}>{kind === 'faction_lock' ? '允许自行换阵营' : '关闭'}</option
						></select
					></label
				>{/if}
		</div>
		{#if kind === 'weapon_restriction' && actionEnabled}
			<p class="text-sm text-mist-400">
				到期应用此任务保存的限制名单。使用受限来源击杀后的处罚沿用武器限制插件。
			</p>
			<div class="flex flex-wrap gap-4">
				{#each [{ id: 'items', label: '全部手持武器' }, { id: 'vehicles', label: '全部载具' }, { id: 'buildables', label: '全部建造物' }] as group}<label
						class="flex gap-2"
						><input
							type="checkbox"
							value={group.id}
							bind:group={selectedGroups}
						/>{group.label}</label
					>{/each}
			</div>
			<input
				class="w-full"
				aria-label="搜索受限来源"
				placeholder="搜索武器名称或来源 ID"
				bind:value={search}
			/>
			<div class="max-h-48 overflow-y-auto rounded border border-white/10 p-2">
				{#each choices as choice}<label class="flex gap-2"
						><input type="checkbox" value={choice.cause} bind:group={selected} />{choice.label}
						<span class="text-xs text-mist-400">{choice.cause}</span></label
					>{/each}
			</div>
			<p class="text-xs text-mist-400">已选择 {selected.length} 个来源；最多显示100个搜索结果。</p>
			<label class="block"
				>补充来源 ID（每行一个）<textarea class="mt-1 w-full" rows="2" bind:value={custom}
				></textarea></label
			>
		{:else if kind === 'trigger'}
			<label class="block"
				>现有自动化规则<select class="mt-1 w-full" required bind:value={triggerId}
					><option value="">请选择规则</option>{#each triggers as trigger}<option value={trigger.id}
							>{trigger.name}</option
						>{/each}</select
				></label
			>
		{:else if kind === 'broadcast'}<label class="block"
				>广播内容<textarea
					class="mt-1 w-full"
					required
					maxlength="200"
					rows="3"
					bind:value={message}></textarea></label
			>
		{:else if kind === 'faction_lock'}<p class="text-sm text-mist-400">
				复用“禁止自行换边”插件，保留已设置的保护时间和阵营容量。管理员授权的调队仍遵循该插件现有机制。
			</p>{/if}
		<button class="btn-primary" disabled={busy}>创建计划任务</button>
	</form>
	{#if error}<p class="text-err" role="alert">{error}</p>{/if}
	<div class="flex items-center justify-between">
		<h3 class="font-semibold">任务与执行记录</h3>
		<button class="btn" disabled={busy} onclick={() => change({ operation: 'clear_finished' })}
			>清除完成记录</button
		>
	</div>
	<p class="text-xs text-mist-400">最多100项未完成任务，保留最近100项完成记录，每页20条。</p>
	<div class="overflow-x-auto">
		<table class="w-full text-sm">
			<thead><tr><th>任务</th><th>时机</th><th>项目</th><th>状态／结果</th><th>操作</th></tr></thead
			><tbody>
				{#each shown as task (task.id)}<tr
						><td>{task.name}</td><td
							>{task.when.kind === 'at' ? fmtTime(task.when.at) : '当前对局后的新一局'}</td
						><td>{scheduleActionLabel(task.action)}</td><td
							>{states[task.deliveryState ?? task.state] ?? task.state}
							<div class="text-xs text-mist-400">{task.deliveryOutcome || task.outcome}</div></td
						><td
							>{#if ['pending', 'queued'].includes(task.state)}<button
									class="btn"
									disabled={busy}
									onclick={() => change({ operation: 'cancel', taskId: task.id })}>取消</button
								>{/if}</td
						></tr
					>{:else}<tr><td colspan="5" class="p-4 text-center text-mist-400">尚无计划任务</td></tr
					>{/each}
			</tbody>
		</table>
	</div>
	<div class="flex items-center gap-3">
		<button class="btn" disabled={page <= 1} onclick={() => page--}>上一页</button><span
			>{Math.min(page, pages)} / {pages} · 共 {sorted.length} 条</span
		><button class="btn" disabled={page >= pages} onclick={() => page++}>下一页</button>
	</div>
</section>
