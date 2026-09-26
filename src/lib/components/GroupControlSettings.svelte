<script lang="ts">
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import type { groupControlView } from '$lib/server/group-control';
	let { data, serverId }: { data: Awaited<ReturnType<typeof groupControlView>>; serverId: string } =
		$props();
	let config = $state(untrack(() => ({ ...data.config })));
	let busy = $state(false),
		message = $state('');
	async function submit(operation: 'save' | 'scan') {
		busy = true;
		message = '';
		try {
			await api('POST', `/api/servers/${serverId}/group-control`, { operation, config });
			await invalidateAll();
			message = operation === 'save' ? '配置已保存' : '名单已更新；AI参与时可稍后刷新查看理由';
		} catch (e) {
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	const labels = {
		possible_group: '疑似统一标记',
		uncertain: '无法确定',
		likely_coincidence: '可能为巧合'
	};
</script>

<section class="space-y-4 panel p-5">
	<h2 class="text-lg font-semibold">组队控制 · 疑似大队识别</h2>
	<p class="text-sm text-mist-300">
		根据同阵营昵称前缀筛选名单，仅供管理员查看，不踢出、不调队、不计入作弊分。昵称相似不能证明实际组队。
	</p>
	<div class="grid gap-4 md:grid-cols-3">
		<label
			>执行方式<select class="mt-2 input w-full" bind:value={config.mode}
				><option value="off">不执行（默认）</option><option value="manual">手动扫描</option><option
					value="auto">自动扫描并列出名单</option
				></select
			></label
		>
		<label
			>识别模式<select class="mt-2 input w-full" bind:value={config.engine}
				><option value="structured">结构化模式</option><option value="ai">AI参与模式</option
				></select
			></label
		>
		<label
			>昵称前缀字符数<input
				class="mt-2 input w-full"
				type="number"
				min="2"
				max="16"
				step="1"
				bind:value={config.prefixLength}
			/></label
		>
		<label
			>相似度严格大于（%）<input
				class="mt-2 input w-full"
				type="number"
				min="0"
				max="99"
				bind:value={config.similarityPercent}
			/></label
		>
		<label
			>同组至少多少人触发<input
				class="mt-2 input w-full"
				type="number"
				min="2"
				max="100"
				step="1"
				bind:value={config.minPlayers}
			/></label
		>
	</div>
	<p class="text-sm text-mist-300">
		忽略大小写、空格、标点与特殊符号，保留文字和数字；不足指定长度的昵称不参与。默认前4字符、相似度＞70%（4字符中替换1个为75%）。每组所有成员两两达标；例如“超过3人”请填4人。
	</p>
	{#if config.engine === 'ai'}<p class="text-sm text-mist-300">
			复用风控 AI
			审查的接口、密钥与模型。扫描时向该接口发送候选昵称、SteamID、阵营、前缀与相似度JSON；AI只补充理由，不增删规则名单。相同名单复用结果，遵守组织请求间隔和每日额度。
		</p>{/if}
	<div class="flex flex-wrap gap-3">
		<button class="btn" disabled={busy} onclick={() => submit('save')}>保存配置</button>
		<button class="btn" disabled={busy || data.config.mode === 'off'} onclick={() => submit('scan')}
			>按已保存配置扫描</button
		>
		<button class="btn" disabled={busy} onclick={() => invalidateAll()}>刷新结果</button>
	</div>
	{#if message}<p role="status">{message}</p>{/if}
	{#if data.scan}
		<p class="text-sm text-mist-300">
			最近扫描：{fmtTime(data.scan.scannedAt)} · {data.scan.groups.length}组 · 前{data.scan.config
				.prefixLength}字符 / ＞{data.scan.config.similarityPercent}% / 至少{data.scan.config
				.minPlayers}人。此为当时名单，玩家可能已离线或换阵营。自动模式每65秒最多扫描一次；刷新页面查看最新结果。
		</p>
		{#if data.scan.aiStatus === 'pending'}<p>
				AI理由生成中；重启中断时等待下次扫描。
			</p>{:else if data.scan.aiStatus === 'unavailable'}<p>
				AI暂不可用（请检查配置、额度或输出格式）；以下规则识别结果仍有效。
			</p>{/if}
		{#each data.scan.groups as group (group.id)}
			{@const advice = data.scan.aiResult?.groups.find((g) => g.id === group.id)}
			<div class="rounded border border-mist-700 p-4">
				<h3 class="font-semibold">
					{group.faction} · {group.members.length}人 · 组内最低相似度 {group.minimumSimilarity.toFixed(
						1
					)}%
				</h3>
				{#each group.members as player (player.steamId)}<p>
						<a class="underline" href={`/server/${serverId}/players/${player.steamId}`}
							>{player.name}</a
						>
						· {player.steamId} · 前缀：{player.prefix}
					</p>{/each}
				{#if advice}<p class="mt-2">AI辅助：{labels[advice.assessment]}。{advice.reason}</p>{/if}
			</div>
		{:else}<p>该次扫描未发现达到人数与相似度条件的组。</p>{/each}
	{:else}<p>尚未扫描。保存配置后手动扫描，或等待自动扫描。</p>{/if}
</section>
