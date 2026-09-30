<script lang="ts">
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	let {
		orgId,
		initial,
		runs
	}: {
		orgId: string;
		initial: {
			revision: string;
			developerEnabled: boolean;
			url: string;
			autoPunishEnabled: boolean;
			maxActionsPerHour: number;
			cooldownSeconds: number;
			p98: number;
			p99: number;
			intervalSeconds: number;
			hasToken: boolean;
			modelId: string;
			referenceSamples: number;
		};
		runs: Record<string, unknown>[];
	} = $props();
	let config = $state(untrack(() => ({ ...initial }))),
		token = $state(''),
		busy = $state(false),
		message = $state('');
	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		message = '';
		try {
			const result = await api<{ config: typeof initial }>(
				'PUT',
				`/api/orgs/${orgId}/integrity/model`,
				{ ...config, token: token || undefined }
			);
			config = result.config;
			token = '';
			message = '配置已保存；在上方选择“仅依赖模型”后开始独立评估。';
			await invalidateAll();
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	async function test() {
		busy = true;
		try {
			const r = await api<{ message: string }>('POST', `/api/orgs/${orgId}/integrity/model`, {});
			message = r.message;
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<section class="border-line mt-5 space-y-4 rounded border p-4" aria-label="开发者模型 A测">
	<h3 class="font-semibold">开发者模型 · A测</h3>
	<p class="text-sm text-mist-400">
		Anomaly Transformer Epoch 58 的 30 分钟时序适配模型，每 30 秒采样一个点，每 30
		分钟评估一次，通过 HTTP 与面板通信。启用后可选择“仅依赖模型”，跳过五专家投票。P98 自动踢出；P99
		优先执行 24 小时临时隔离。异常分数不是作弊概率。
	</p>
	<form onsubmit={save} class="space-y-3">
		<label class="flex gap-2"
			><input
				type="checkbox"
				bind:checked={config.developerEnabled}
				disabled={busy}
			/>启用开发者状态</label
		>
		<label class="block text-sm"
			>模型 API 根地址<input
				class="mt-1 input"
				type="url"
				placeholder="http://model-host:8091"
				bind:value={config.url}
				disabled={busy}
			/></label
		>
		<label class="block text-sm"
			>API 令牌（{config.hasToken ? '已保存，留空保留' : '尚未设置'}）<input
				class="mt-1 input"
				type="password"
				autocomplete="new-password"
				bind:value={token}
				disabled={busy}
			/></label
		>
		<label class="flex gap-2"
			><input
				type="checkbox"
				bind:checked={config.autoPunishEnabled}
				disabled={busy}
			/>自动处罚：P98 踢出 / P99 隔离 24 小时</label
		>
		<label class="block text-sm"
			>同玩家处罚冷却（秒）<input
				class="mt-1 input"
				type="number"
				min="60"
				max="86400"
				bind:value={config.cooldownSeconds}
				required
				disabled={busy}
			/></label
		>
		<p class="text-sm">
			P98：{config.p98.toFixed(6)} · P99：{config.p99.toFixed(6)}（固定模型参考分布）
		</p>
		<div class="grid gap-3 sm:grid-cols-2">
			<label class="block text-sm"
				>每组织每小时最多处罚<input
					class="mt-1 input"
					type="number"
					min="1"
					max="100"
					bind:value={config.maxActionsPerHour}
					required
					disabled={busy}
				/></label
			>
			<label class="block text-sm"
				>每玩家请求间隔（秒）<input
					class="mt-1 input"
					type="number"
					min="1800"
					max="86400"
					bind:value={config.intervalSeconds}
					required
					disabled={busy}
				/></label
			>
		</div>
		<p class="text-xs text-mist-400">
			百分位基于 {config.referenceSamples} 个未标注参考窗口，并非人工确认的正常玩家分布。输入需同局连续
			30 分钟（60 个有序采样点）；定时评估在线玩家，换局重新累计。数据不足、断连或版本不符时显示原因，不回退专家。HTTP
			明文连接请放在可信网络内，跨公网可使用 HTTPS。
		</p>
		<div class="flex gap-2">
			<button class="btn btn-primary" disabled={busy}>保存模型配置</button><button
				class="btn"
				type="button"
				onclick={test}
				disabled={busy || !config.hasToken}>测试已保存的连接</button
			><button class="btn" type="button" onclick={() => invalidateAll()} disabled={busy}
				>刷新结果</button
			>
		</div>
	</form>
	{#if message}<p role="status" class="text-sm">{message}</p>{/if}
	<div class="overflow-auto">
		<table class="w-full text-left text-sm">
			<thead
				><tr
					><th>玩家 SteamID64</th><th>对局</th><th>状态</th><th>分数 / 阈值</th><th
						>处罚 / 执行状态</th
					><th>时间</th></tr
				></thead
			><tbody>
				{#each runs as run}<tr
						><td>{String(run.steam_id)}</td><td>{String(run.match_id)}</td><td
							>{String(run.state)}{run.anomalous === true ? ' · 超过实验阈值' : ''}<br />{String(
								run.data_reason ?? ''
							)}{run.required_buckets
								? ' · 观测 ' +
									String(run.observed_buckets) +
									'/' +
									String(run.required_buckets) +
									' 点'
								: ''}</td
						><td
							>{run.score == null ? '—' : Number(run.score).toFixed(6)} / {String(
								run.threshold
							)}</td
						><td
							>{String(run.action ?? '—')} / {String(run.action_state ?? '—')}<br />{String(
								run.action_reason ?? ''
							)}{run.expires_at
								? ' · 到期 ' + new Date(String(run.expires_at)).toLocaleString()
								: ''}</td
						><td>{new Date(String(run.created_at)).toLocaleString()}</td></tr
					>{:else}<tr
						><td colspan="6">尚无模型评估。请配置 API、选择模型模式，并等待有效数据。</td></tr
					>{/each}
			</tbody>
		</table>
	</div>
</section>
