<script lang="ts">
	import { api, errorMessage } from '$lib/api';
	import { invalidateAll } from '$app/navigation';
	import { untrack } from 'svelte';
	import { RESTRICTION_GROUPS } from '$lib/weapon-restriction-policy';
	import { causeLabel } from '$lib/causes';
	import { fmtTime } from '$lib/format';
	import type { weaponRestrictionView } from '$lib/server/weapon-restrictions';
	let {
		serverId,
		data,
		feed
	}: { serverId: string; data: Awaited<ReturnType<typeof weaponRestrictionView>>; feed: boolean } =
		$props();
	let enabled = $state(untrack(() => data.rule.enabled)),
		selected = $state<string[]>(untrack(() => [...data.rule.causes])),
		groups = $state<string[]>(untrack(() => [...data.rule.groups]));
	let search = $state(''),
		custom = $state(''),
		busy = $state(false),
		message = $state('');
	let choices = $derived(
		data.catalogue.filter((c) =>
			(c.label + ' ' + c.cause).toLowerCase().includes(search.toLowerCase())
		)
	);
	function add() {
		const values = custom
			.split(/\r?\n/)
			.map((v) => v.trim())
			.filter(Boolean);
		selected = [...new Set([...selected, ...values])];
		custom = '';
	}
	async function save() {
		busy = true;
		message = '';
		try {
			await api('POST', `/api/servers/${serverId}/weapon-restrictions`, {
				enabled,
				causes: selected,
				groups
			});
			await invalidateAll();
			message = '已保存；从现在开始按新规则计数。';
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	const states: Record<string, string> = {
		executing: '执行中',
		delivered: '已执行',
		error: '失败／结果未知',
		skipped: '已取消'
	};
</script>

<section class="mt-4 space-y-4 panel p-5">
	<h2 class="text-xl font-semibold">武器／载具来源限制</h2>
	<p class="text-sm text-mist-400">
		仅能检测 Kill Feed
		的击杀，无法检测未致死的普通伤害或阻止购买装备。按玩家、当前对局累计：首次受限击杀警告；警告成功至少8秒后再次用任意受限来源击杀则踢出。同一爆炸的多杀不会立即升级。换图或保存新规则后重新计数；重新加入继续违规仍会踢出（间隔至少60秒）。
	</p>
	{#if !feed}<p class="text-warn">尚未配置 Kill Feed，启用后也无法执行；请先配置击杀回传。</p>{/if}
	<label class="flex gap-2"><input type="checkbox" bind:checked={enabled} />启用武器限制</label>
	<fieldset class="space-y-2 rounded border border-white/10 p-3">
		<legend>整类限制（包括以后新出现的同类来源）</legend>{#each RESTRICTION_GROUPS as group}<label
				class="flex gap-2"
				><input type="checkbox" value={group.id} bind:group={groups} />{group.label}</label
			>{/each}
	</fieldset>
	<label class="block"
		>按名称或来源 ID 查找<input
			class="mt-1 input w-full"
			bind:value={search}
			placeholder="枪支、手雷、载具武器或完整 ID"
		/></label
	>
	<div class="max-h-64 space-y-2 overflow-auto rounded border border-white/10 p-3">
		{#each choices as item}<label class="flex gap-2"
				><input type="checkbox" value={item.cause} bind:group={selected} /><span
					>{item.label}<small class="block break-all text-mist-400">{item.cause}</small></span
				></label
			>{/each}
	</div>
	<p class="text-xs text-mist-400">
		列表合并已知来源和本服近30天出现过的来源；没有冒充完整游戏装备清单。可选择所有类型，也可补充尚未出现的精确来源
		ID。载具本体和车载武器可能使用不同 ID，请分别选择或使用整类限制。
	</p>
	{#if data.catalogueTruncated}<p>来源数量过多，列表未全部显示，请手动填写 ID。</p>{/if}
	<details>
		<summary>手动添加完整来源 ID（每行一项）</summary><textarea
			class="my-2 input w-full"
			rows="3"
			bind:value={custom}></textarea><button class="btn" onclick={add}>加入待保存列表</button>
	</details>
	<details>
		<summary>已选择 {selected.length} 项精确来源</summary>{#each selected as cause}<div
				class="my-1 flex gap-2"
			>
				<span class="break-all">{causeLabel(cause)} · {cause}</span><button
					class="btn"
					onclick={() => (selected = selected.filter((c) => c !== cause))}>移除</button
				>
			</div>{/each}
	</details>
	<button class="btn-primary" disabled={busy} onclick={save}
		>{busy ? '保存中…' : '保存武器限制'}</button
	><span role="status">{message}</span>
	<h3 class="font-semibold">最近50条执行记录</h3>
	<div class="table-wrap">
		<table>
			<thead><tr><th>时间</th><th>玩家</th><th>来源</th><th>操作</th><th>结果</th></tr></thead
			><tbody
				>{#each data.events as event}<tr
						><td>{fmtTime(event.createdAt)}</td><td
							><a href={`/server/${serverId}/players/${event.steamId}`}
								>{event.playerName} · {event.steamId.slice(-6)}</a
							></td
						><td title={event.cause}>{causeLabel(event.cause)}</td><td
							>{event.action === 'warn' ? '警告' : '踢出'}</td
						><td>{states[event.state] ?? event.state}：{event.reason}</td></tr
					>{:else}<tr><td colspan="5">尚无记录</td></tr>{/each}</tbody
			>
		</table>
	</div>
</section>
