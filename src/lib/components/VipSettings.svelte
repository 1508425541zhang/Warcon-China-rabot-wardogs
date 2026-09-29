<script lang="ts">
	import { untrack } from 'svelte';
	import { api, errorMessage } from '$lib/api';
	import type { VipSettings } from '$lib/server/qq/vip';
	let {
		initial,
		servers
	}: { initial: VipSettings; servers: { id: string; name: string; orgName: string }[] } = $props();
	let model = $state(untrack(() => structuredClone(initial)));
	let busy = $state(false),
		dirty = $state(false),
		message = $state(''),
		failed = $state(false);
	function add() {
		model.entries.push({
			serverId: servers[0]?.id || '',
			steamId: '',
			enabled: true,
			reserve: true,
			allowOverkill: false,
			whitelist: false,
			note: ''
		});
		dirty = true;
	}
	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		message = '';
		failed = false;
		try {
			const result = await api<{ vips: VipSettings }>(
				'PUT',
				'/api/admin/qq/vips',
				$state.snapshot(model)
			);
			model = result.vips;
			dirty = false;
			message = 'VIP 名单已保存。预留位等待名单同步，其他权益在下一次执行检查时生效。';
		} catch (e) {
			failed = true;
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	async function reload() {
		busy = true;
		message = '';
		failed = false;
		try {
			model = (await api<{ vips: VipSettings }>('GET', '/api/admin/qq/vips')).vips;
			dirty = false;
		} catch (e) {
			failed = true;
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<section class="mt-6 space-y-4 panel" aria-label="VIP 设置">
	<div>
		<h2 class="text-xl font-semibold">VIP 设置</h2>
		<p class="mt-1 text-sm text-mist-400">
			按服务器绑定 SteamID64，三个权益独立选择。此名单独立保存，不受 QQ 机器人开关影响。
		</p>
	</div>
	<ul class="space-y-1 text-sm text-mist-400">
		<li>预留位：同步游戏预留位名单，撤销 VIP 不影响其他来源的预留位；部分游戏版本需重启后生效。</li>
		<li>允许超杀：免除 KD、KPM 数值上限，金钱限制仍适用。</li>
		<li>
			白名单：允许在自动风控隔离期间进服，免除自动风险踢人、自动风控处罚及数值限制。记录与人工审核保留；人工封禁和游戏原生封禁仍需管理员单独解除。
		</li>
	</ul>
	{#if message}<p
			role={failed ? 'alert' : 'status'}
			class="rounded border p-3 text-sm {failed
				? 'border-red-400/50 text-red-300'
				: 'border-emerald-400/40 text-emerald-200'}"
		>
			{message}
		</p>{/if}
	<form onsubmit={save} oninput={() => (dirty = true)} onchange={() => (dirty = true)}>
		<fieldset disabled={busy} class="space-y-4">
			{#each model.entries as entry, index}
				<div class="space-y-3 rounded border border-white/10 p-4">
					<div class="flex justify-between gap-3">
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={entry.enabled} />启用 VIP</label
						><button
							type="button"
							class="text-sm text-red-300 underline"
							onclick={() => {
								model.entries.splice(index, 1);
								dirty = true;
							}}>移除</button
						>
					</div>
					<div class="grid gap-3 md:grid-cols-2">
						<label class="space-y-1"
							><span class="label-sm">服务器</span><select
								class="input w-full"
								required
								bind:value={entry.serverId}
								><option value="" disabled>选择服务器</option>{#each servers as s}<option
										value={s.id}>{s.orgName} / {s.name}</option
									>{/each}</select
							></label
						><label class="space-y-1"
							><span class="label-sm">SteamID64</span><input
								class="input w-full"
								bind:value={entry.steamId}
								inputmode="numeric"
								pattern={'[0-9]{17}'}
								maxlength="17"
								required
								placeholder="17 位 SteamID64"
							/></label
						>
					</div>
					<div class="flex flex-wrap gap-5">
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={entry.reserve} />预留位</label
						><label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={entry.allowOverkill} />允许超杀</label
						><label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={entry.whitelist} />白名单</label
						>
					</div>
					<label class="block space-y-1"
						><span class="label-sm">备注（可选）</span><input
							class="input w-full"
							bind:value={entry.note}
							maxlength="200"
							placeholder="管理员备注"
						/></label
					>
				</div>
			{:else}<p class="text-sm text-mist-400">暂无 VIP。添加玩家后选择权益并保存。</p>{/each}
			<div class="flex flex-wrap gap-3">
				<button
					type="button"
					class="btn"
					onclick={add}
					disabled={!servers.length || model.entries.length >= 500}>添加 VIP</button
				><button type="submit" class="btn btn-primary">保存 VIP 名单</button><button
					type="button"
					class="btn"
					onclick={reload}>重新加载并放弃 VIP 修改</button
				><span class="self-center text-sm text-mist-400"
					>{dirty ? 'VIP 有未保存的修改' : `${model.entries.length} 项 VIP`}</span
				>
			</div>
		</fieldset>
	</form>
</section>
