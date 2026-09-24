<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	type Setting = (typeof data.settings)[number];
	/** edits by key, in the unit shown (seconds for millisecond settings) */
	let edits = $state<Record<string, string>>({});
	let busy = $state(false);

	const GROUPS: { id: Setting['group']; title: string; blurb: string }[] = [
		{
			id: 'observation',
			title: '采样频率',
			blurb:
				'设置工作进程检查服务器的频率。“有人查看”表示页面已打开，“繁忙”表示服务器有玩家。游戏监听器决定最短间隔；间隔越短，数据越新。'
		},
		{
			id: 'delivery',
			title: '触发动作发送',
			blurb: '设置自动化动作的发送方式，以及过期动作的丢弃时间。'
		},
		{
			id: 'housekeeping',
			title: '数据维护',
			blurb: '不属于服务器采样的数据库写入。历史记录不会删除。'
		},
		{
			id: 'accounts',
			title: '账号与登录',
			blurb:
				'设置登录安全规则的适用范围及宽限期。规则要求两种独立登录方式，使用密码时还需第二因素；详见账号页面。'
		}
	];

	const shown = (s: Setting, v = s.value) => (s.unit === 'ms' ? String(v / 1000) : String(v));
	const unitLabel = (s: Setting) => (s.unit === 'ms' ? '秒' : s.unit === 'days' ? '天' : '');
	const bounds = (s: Setting) =>
		s.unit === 'choice'
			? ''
			: s.unit === 'ms'
				? `${s.min / 1000}–${s.max / 1000} 秒`
				: `${s.min}–${s.max}`;
	const optionLabel = (s: Setting, v: number) =>
		s.options?.find((o) => o.value === v)?.label ?? String(v);
	const value = (s: Setting) => (s.key in edits ? edits[s.key] : shown(s));
	const dirty = (s: Setting) => s.key in edits && edits[s.key] !== shown(s);

	async function save() {
		const values: Record<string, number> = {};
		for (const s of data.settings) {
			if (!dirty(s)) continue;
			const n = Number(edits[s.key]);
			if (!Number.isFinite(n)) {
				toast(`${s.label}：请输入数字。`, 'err');
				return;
			}
			values[s.key] = s.unit === 'ms' ? Math.round(n * 1000) : Math.round(n);
		}
		if (!Object.keys(values).length) return;
		busy = true;
		try {
			await api('PUT', '/api/settings', { values });
			toast('设置已保存，工作进程将在十秒内读取。', 'ok');
			edits = {};
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function reset(s: Setting) {
		busy = true;
		try {
			await api('PUT', '/api/settings', { reset: [s.key] });
			delete edits[s.key];
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>设置 · 站点管理 · {data.appName}</title></svelte:head>

{#each GROUPS as g (g.id)}
	<div class="mb-4 panel">
		<span class="label-sm">{g.title}</span>
		<p class="mb-3 text-[13px] text-mist-400">{g.blurb}</p>
		<div class="table-wrap">
			<table>
				<thead><tr><th>设置</th><th>值</th><th>已允许</th><th></th></tr></thead>
				<tbody>
					{#each data.settings.filter((s) => s.group === g.id) as s (s.key)}
						<tr>
							<td>
								<div class="font-medium">{s.label}</div>
								<div class="text-[12.5px] text-mist-500">{s.help}</div>
							</td>
							<td class="whitespace-nowrap">
								{#if s.options}
									<select
										class="input pr-[30px]"
										value={value(s)}
										onchange={(e) => (edits[s.key] = (e.target as HTMLSelectElement).value)}
									>
										{#each s.options as o (o.value)}<option value={String(o.value)}
												>{o.label}</option
											>{/each}
									</select>
								{:else}
									<span class="join">
										<input
											class="input w-28"
											type="number"
											step={s.unit === 'ms' ? 0.5 : 1}
											value={value(s)}
											oninput={(e) => (edits[s.key] = (e.target as HTMLInputElement).value)}
										/>
										<span class="pointer-events-none btn btn-ghost">{unitLabel(s)}</span>
									</span>
								{/if}
								{#if dirty(s)}<Badge tone="warn" class="ml-1">未保存</Badge>{/if}
							</td>
							<td class="whitespace-nowrap text-mist-500">{bounds(s)}</td>
							<td class="whitespace-nowrap">
								{#if s.stored}
									<button class="btn btn-sm" disabled={busy} onclick={() => reset(s)}
										>恢复为 {s.options
											? optionLabel(s, s.default)
											: shown(s, s.default) + unitLabel(s)}</button
									>
								{:else}<span class="text-[12.5px] text-mist-600">默认</span>{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
{/each}

<div class="flex items-center gap-3">
	<button class="btn btn-primary" disabled={busy || !data.settings.some(dirty)} onclick={save}
		>保存修改</button
	>
	<span class="text-[13px] text-mist-500">修改会记入审计日志，无需重启即可生效。</span>
</div>
