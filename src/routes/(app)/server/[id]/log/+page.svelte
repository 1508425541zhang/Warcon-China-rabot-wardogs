<script lang="ts">
	import { rconGet, errorMessage } from '$lib/api';
	import { poll } from '$lib/poll';
	import { toast } from '$lib/toast.svelte';
	import { matches } from '$lib/table.svelte';
	import type { LogEntry } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	// The peers' addresses come blank to everyone else, so the column is theirs alone.
	let siteOwner = $derived(data.user.role === 'owner');

	let tail = $state(50);
	let auto = $state(true);
	let entries = $state<LogEntry[]>([]);
	let loaded = $state(false);
	let search = $state('');
	let rows = $derived(
		entries.filter((e) => matches(search, e.peer, e.sessionId, e.event, e.detail))
	);

	const EVENT_CLASS: Record<string, string> = {
		AUTH_OK: 'text-ok',
		HTTP: 'text-mist-400',
		AUTH_FAIL: 'text-danger',
		REJECT: 'text-danger',
		COMMAND: 'text-warn'
	};

	async function refresh() {
		try {
			const d = await rconGet<{ entries: LogEntry[] }>(id, 'serverLog', { limit: tail });
			entries = [...d.entries].reverse();
			loaded = true;
		} catch (err) {
			toast(errorMessage(err), 'err');
		}
	}
	$effect(() => poll(() => (auto ? refresh() : undefined), 5000));
</script>

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-3">
		<span class="label-sm mb-0">尾部</span>
		<select class="input w-24" bind:value={tail} onchange={refresh}>
			{#each [25, 50, 100, 200, 500] as n (n)}<option value={n}>{n}</option>{/each}
		</select>
		<label class="inline-flex items-center gap-2 text-[13px]"
			><input type="checkbox" bind:checked={auto} /> 自动刷新</label
		>
		<button class="btn btn-sm" onclick={refresh}>刷新</button>
		<input
			class="input w-full sm:w-64"
			type="search"
			placeholder={siteOwner ? '按来源、事件、详情筛选…' : '按事件和详情筛选…'}
			aria-label="筛选日志"
			bind:value={search}
		/>
		<span class="ml-auto text-[12.5px] text-mist-600"
			>{search.trim() ? `${rows.length} of ` : ''}{entries.length} 条记录（最新优先）</span
		>
	</div>
	<div class="table-wrap">
		<table>
			<thead
				><tr
					><th>时间戳（UTC）</th>{#if siteOwner}<th>对端</th>{/if}<th>场次</th><th>事件</th><th
						>详情</th
					></tr
				></thead
			>
			<tbody>
				{#each rows as e, i (i)}
					<tr>
						<td class="font-mono text-[12px] whitespace-nowrap">{e.timestampUtc}</td>
						{#if siteOwner}<td class="font-mono text-[12px]">{e.peer}</td>{/if}
						<td class="font-mono text-[12px] text-mist-400">{e.sessionId}</td>
						<td class="font-semibold {EVENT_CLASS[e.event] || ''}">{e.event}</td>
						<td class="max-w-[480px] font-mono text-[12px] break-words">{e.detail}</td>
					</tr>
				{:else}
					<tr
						><td colspan={siteOwner ? 5 : 4} class="py-6 text-center text-mist-600"
							>{!loaded ? 'Loading…' : entries.length ? '没有符合条件的记录。' : 'No entries.'}</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="note">
		这是游戏服务器自己的 RCON 监听日志（GET
		/v1/audit），包含所有客户端的连接、认证和命令，也包括其他管理工具。面板自身的操作记录请查看“审计”。
	</p>
</div>
