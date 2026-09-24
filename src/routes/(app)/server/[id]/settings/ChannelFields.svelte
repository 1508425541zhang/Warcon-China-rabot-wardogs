<script lang="ts">
	// What a channel carries and how its card looks. The connect form and the edit dialog share
	// these fields so both ask the same question in the same words.
	import { STATUS_STYLE_LABELS, STATUS_STYLES, type StatusStyle } from '$lib/status-styles';
	import CardOptions from '$lib/components/CardOptions.svelte';
	import type { FeatureSet } from '$lib/features';

	type Carry = 'card' | 'teamkills' | 'both';
	let {
		name,
		carry = $bindable(),
		style = $bindable(),
		interval = $bindable(),
		linkStatus = $bindable(),
		linkLeaderboard = $bindable(),
		linkPanel = $bindable(),
		features
	}: {
		name: string;
		carry: Carry;
		style: StatusStyle;
		interval: number;
		linkStatus: boolean;
		linkLeaderboard: boolean;
		linkPanel: boolean;
		features: FeatureSet;
	} = $props();
</script>

<div>
	<span class="field-label">频道发送内容</span>
	<div class="flex flex-wrap gap-x-6 gap-y-2">
		<label class="flex items-center gap-2 text-[13px]"
			><input type="radio" {name} value="card" bind:group={carry} /> 实时状态卡片</label
		>
		<label class="flex items-center gap-2 text-[13px]"
			><input type="radio" {name} value="teamkills" bind:group={carry} /> 队友击杀</label
		>
		<label class="flex items-center gap-2 text-[13px]"
			><input type="radio" {name} value="both" bind:group={carry} /> 两者</label
		>
	</div>
</div>
{#if carry !== 'teamkills'}
	<label class="block sm:w-60"
		><span class="field-label">卡片样式</span><select class="input" bind:value={style}>
			{#each STATUS_STYLES as st (st)}<option value={st}>{st}</option>{/each}
		</select></label
	>
	<p class="note">{STATUS_STYLE_LABELS[style]}</p>
	<CardOptions bind:interval bind:linkStatus bind:linkLeaderboard bind:linkPanel {features} />
{/if}
{#if carry !== 'card'}<p class="note">
		每次队友击杀发送一条消息，以击杀事件为准（在“配置”选项卡中设置）。
	</p>{/if}
