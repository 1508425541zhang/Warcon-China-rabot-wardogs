<script lang="ts">
	// How often a status card is edited and what it links to. The server's Settings tab and the
	// org page's webhook dialog share these fields so both ask in the same words.
	import type { FeatureSet } from '$lib/features';

	let {
		interval = $bindable(),
		linkStatus = $bindable(),
		linkLeaderboard = $bindable(),
		linkPanel = $bindable(),
		/** the one server's public pages, when the card is for one server; null across an org */
		features = null
	}: {
		interval: number;
		linkStatus: boolean;
		linkLeaderboard: boolean;
		linkPanel: boolean;
		features?: FeatureSet | null;
	} = $props();

	const INTERVALS = [
		[30, '30 seconds'],
		[45, '45 seconds'],
		[60, '1 minute'],
		[90, '90 seconds'],
		[120, '2 minutes'],
		[180, '3 minutes'],
		[300, '5 minutes']
	] as const;
	const off = (on: boolean | undefined) => on === false;
</script>

<label class="block sm:w-60"
	><span class="field-label">刷新</span><select class="input" bind:value={interval}>
		{#each INTERVALS as [s, label] (s)}<option value={s}>每 {label}</option>{/each}
	</select></label
>
<div>
	<span class="field-label">卡片上的链接</span>
	<div class="flex flex-wrap gap-x-6 gap-y-2">
		<label class="flex items-center gap-2 text-[13px]"
			><input type="checkbox" bind:checked={linkStatus} /> 实时状态页面{#if off(features?.status)}
				<span class="text-mist-600">（页面已关闭）</span>{/if}</label
		>
		<label class="flex items-center gap-2 text-[13px]"
			><input type="checkbox" bind:checked={linkLeaderboard} />
			排行榜{#if off(features?.leaderboards)}
				<span class="text-mist-600">（页面已关闭）</span>{/if}</label
		>
		<label class="flex items-center gap-2 text-[13px]"
			><input type="checkbox" bind:checked={linkPanel} /> 管理面板（需登录）</label
		>
	</div>
	<p class="note">
		标题打开第一个链接，其余链接显示在卡片下方。仅当服务器启用对应公开页面时才发送公开链接（在“公开页面”中设置），避免将玩家带到登录页面。修改至少等待一个刷新周期；多个服务器共用同一
		Webhook 时可能更久。
	</p>
</div>
