<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, errorMessage, qs } from '$lib/api';
	import { fmtTime, toDatetimeLocal } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import type { AuditRow } from '$lib/server/audit';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let extra = $state<AuditRow[]>([]);
	let nextBefore = $state<number | null>(null);
	let loadingMore = $state(false);
	// Reset the appended pages whenever the server-loaded page changes.
	$effect(() => {
		data.entries;
		extra = [];
		nextBefore = data.nextBefore;
	});
	let rows = $derived([...data.entries, ...extra]);

	let f = $state({ server: '', actor: '', action: '', outcome: '', q: '', from: '', to: '' });
	$effect(() => {
		const d = data.filters;
		f = {
			server: d.serverId || '',
			actor: d.actorId || '',
			action: d.action || '',
			outcome: d.outcome || '',
			q: d.q || '',
			from: toDatetimeLocal(d.from),
			to: toDatetimeLocal(d.to)
		};
	});

	const query = () => ({
		server: f.server,
		actor: f.actor,
		action: f.action,
		outcome: f.outcome,
		q: f.q.trim(),
		from: f.from ? new Date(f.from).toISOString() : '',
		to: f.to ? new Date(f.to).toISOString() : ''
	});
	let timer: ReturnType<typeof setTimeout> | undefined;
	function apply() {
		void goto(`/audit${qs(query())}`, { keepFocus: true, noScroll: true, replaceState: true });
	}
	function applyDebounced() {
		clearTimeout(timer);
		timer = setTimeout(apply, 300);
	}
	async function more() {
		if (!nextBefore) return;
		loadingMore = true;
		try {
			const d = await api<{ entries: AuditRow[]; nextBefore: number | null }>(
				'GET',
				`/api/audit${qs({ ...query(), before: nextBefore, limit: 100 })}`
			);
			extra.push(...d.entries);
			nextBefore = d.nextBefore;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			loadingMore = false;
		}
	}
	const exportUrl = (format: string) => `/api/audit/export${qs({ ...query(), format })}`;
	const outcomeClass = (o: string) =>
		o === 'ok' ? 'text-ok' : o === 'denied' ? 'text-warn' : 'text-danger';
</script>

<svelte:head><title>审计记录 · {data.appName}</title></svelte:head>

<h1 class="mb-5 text-xl font-semibold tracking-tight">审计记录</h1>

<div class="mb-4 panel">
	<div class="grid grid-cols-2 gap-2 md:grid-cols-4 xl:grid-cols-8">
		<select class="input" bind:value={f.server} onchange={apply} aria-label="服务器">
			<option value="">全部服务器</option>
			{#each data.servers as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
		</select>
		<select class="input" bind:value={f.actor} onchange={apply} aria-label="操作者">
			<option value="">全部操作者</option>
			{#each data.actors as a (a.actorId)}<option value={a.actorId}
					>{a.actorName || a.actorId}</option
				>{/each}
		</select>
		<select class="input" bind:value={f.action} onchange={apply} aria-label="操作">
			<option value="">全部操作</option>
			{#each data.actions as a (a.action)}<option value={a.action}>{a.category} · {a.action}</option
				>{/each}
		</select>
		<select class="input" bind:value={f.outcome} onchange={apply} aria-label="结果">
			<option value="">全部结果</option>
			<option value="ok">ok</option>
			<option value="error">错误</option>
			<option value="denied">已拒绝</option>
		</select>
		<input
			class="col-span-2 input"
			type="search"
			placeholder="搜索目标、消息或详情…"
			bind:value={f.q}
			oninput={applyDebounced}
		/>
		<input class="input" type="datetime-local" title="从" bind:value={f.from} onchange={apply} />
		<input class="input" type="datetime-local" title="至" bind:value={f.to} onchange={apply} />
	</div>
	<div class="mt-3 flex flex-wrap items-center gap-3">
		<span class="text-[12.5px] text-mist-400">
			{data.user.role === 'owner'
				? '查看全平台的登录、用户变更、服务器变更及游戏服务器命令。'
				: '查看自己的操作，以及所管理服务器上的全部操作。'}
		</span>
		<span class="ml-auto flex items-center gap-2">
			<span class="text-[12.5px] text-mist-600"
				>{rows.length} 条记录{nextBefore ? '（还有更多）' : ''}</span
			>
			<a class="btn btn-sm" href={exportUrl('csv')} target="_blank" rel="noopener">导出 CSV</a>
			<a class="btn btn-sm" href={exportUrl('json')} target="_blank" rel="noopener">导出 JSON</a>
		</span>
	</div>
</div>

<div class="table-wrap">
	<table>
		<thead>
			<tr
				><th>时间</th><th>操作者</th><th>服务器</th><th>操作</th><th>目标</th><th>结果</th><th
					>消息／详情</th
				><th class="num">ms</th></tr
			>
		</thead>
		<tbody>
			{#each rows as r (r.id)}
				<tr>
					<td class="font-mono text-[12px] whitespace-nowrap" title={String(r.ts)}
						>{fmtTime(r.ts)}</td
					>
					<td
						>{#if r.actorName}{r.actorName}{:else}<span class="text-mist-600">—</span>{/if}</td
					>
					<td
						>{#if r.serverName}{r.serverName}{:else}<span class="text-mist-600">—</span>{/if}</td
					>
					<td><span class="chip">{r.action}</span></td>
					<td class="font-mono text-[12px]">{r.target}</td>
					<td class="font-semibold whitespace-nowrap {outcomeClass(r.outcome)}"
						>{r.outcome.toUpperCase()}{r.status ? ` ${r.status}` : ''}</td
					>
					<td class="max-w-[420px]">
						{#if r.message}<div>{r.message}</div>{/if}
						{#if r.detail && Object.keys(r.detail as object).length}
							<details class="text-[12px] text-mist-400">
								<summary class="cursor-pointer">详情</summary>
								<pre
									class="mt-1 max-h-60 overflow-auto rounded-ctl bg-black/40 p-2 font-mono text-[11.5px] whitespace-pre-wrap text-mist-100">{JSON.stringify(
										r.detail,
										null,
										2
									)}</pre>
							</details>
						{/if}
					</td>
					<td class="num font-mono text-[12px] text-mist-600">{r.durationMs ?? ''}</td>
				</tr>
			{:else}
				<tr><td colspan="8" class="py-8 text-center text-mist-600">没有符合条件的审计记录。</td></tr
				>
			{/each}
		</tbody>
	</table>
</div>
{#if nextBefore}
	<div class="mt-4 text-center">
		<button class="btn" onclick={more} disabled={loadingMore}
			>{loadingMore ? 'Loading…' : '加载更多'}</button
		>
	</div>
{/if}
{#if page.url.searchParams.size}
	<div class="mt-3 text-center">
		<a href="/audit" class="text-[12.5px] text-mist-400 underline">清除筛选</a>
	</div>
{/if}
