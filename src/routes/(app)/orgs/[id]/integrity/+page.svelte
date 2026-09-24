<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { toast } from '$lib/toast.svelte';
	import IntegritySettings from '../../../server/[id]/integrity/IntegritySettings.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let lang = $state<'zh' | 'en'>('zh');
	let busy = $state(false);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoKickEnabled = $state(data.enforcement.autoKickEnabled);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoQuarantine24hEnabled = $state(data.enforcement.autoQuarantine24hEnabled);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoQuarantine7dEnabled = $state(data.enforcement.autoQuarantine7dEnabled);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoActionMaxPerHour = $state(data.enforcement.autoActionMaxPerHour);
	// svelte-ignore state_referenced_locally -- editable form starts from the server snapshot.
	let autoActionMaxPercentOnline = $state(data.enforcement.autoActionMaxPercentOnline);
	async function save() {
		const enabling =
			(autoKickEnabled && !data.enforcement.autoKickEnabled) ||
			(autoQuarantine24hEnabled && !data.enforcement.autoQuarantine24hEnabled) ||
			(autoQuarantine7dEnabled && !data.enforcement.autoQuarantine7dEnabled);
		if (
			enabling &&
			!(await confirmDialog(
				'实验性自动处置可能误判。启用后系统可能自动踢出、暂时拒入 24 小时或 7 天。不会自动永久封禁。确认开启？',
				{ okLabel: '确认开启实验性自动处置', danger: true }
			))
		)
			return;
		busy = true;
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(data.orgId)}/integrity/enforcement`, {
				values: {
					autoKickEnabled,
					autoQuarantine24hEnabled,
					autoQuarantine7dEnabled,
					autoActionMaxPerHour,
					autoActionMaxPercentOnline,
					confirmation: enabling ? 'ENABLE_EXPERIMENTAL_INTEGRITY' : undefined
				}
			});
			toast('自动处置设置已保存。', 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function resume() {
		if (
			!(await confirmDialog('确认手动恢复实验性自动处置？请先检查熔断原因。', {
				okLabel: '恢复自动处置',
				danger: true
			}))
		)
			return;
		busy = true;
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(data.orgId)}/integrity/enforcement`, {
				values: { resume: true, confirmation: 'ENABLE_EXPERIMENTAL_INTEGRITY' }
			});
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>组织社区风控设置</title></svelte:head>
<div class="mb-5 flex items-center justify-between gap-3">
	<div>
		<h2 class="text-xl font-semibold text-white">
			{lang === 'zh' ? '组织社区风控设置' : 'Organization Community Integrity'}
		</h2>
		<p class="mt-1 text-sm text-mist-400">
			{lang === 'zh'
				? '这些规则适用于本组织的所有服务器。'
				: 'These rules apply to every server in this organization.'}
		</p>
	</div>
	<button class="btn-quiet btn" onclick={() => (lang = lang === 'zh' ? 'en' : 'zh')}
		>{lang === 'zh' ? 'English' : '简体中文'}</button
	>
</div>
<section class="mb-6 panel p-4">
	<h3 class="text-base font-semibold text-white">
		{lang === 'zh' ? '实验性自动处置' : 'Experimental automatic actions'}
	</h3>
	<p class="my-3 text-sm text-warn">
		{lang === 'zh'
			? '实验性自动处置可能误判。开启后可能自动踢出、24 小时或 7 天暂时拒入；不会自动永久封禁。所有开关默认关闭。'
			: 'Experimental actions can make mistakes. They may kick or temporarily restrict entry for 24 hours or 7 days. Permanent automatic bans are never used. All switches default to off.'}
	</p>
	{#if data.enforcement.autoSuspendedAt}<p class="mb-3 text-sm text-danger">
			{lang === 'zh'
				? '自动处置已熔断，需要管理员手动恢复。'
				: 'Automatic actions are suspended until an owner resumes them.'}
		</p>
		<button class="mb-3 btn btn-danger" disabled={busy} onclick={resume}
			>{lang === 'zh' ? '检查后恢复' : 'Resume after review'}</button
		>{/if}
	<div class="grid gap-3 sm:grid-cols-3">
		<label class="flex items-center gap-2"
			><input type="checkbox" bind:checked={autoKickEnabled} />{lang === 'zh'
				? '自动踢出'
				: 'Automatic kick'}</label
		>
		<label class="flex items-center gap-2"
			><input type="checkbox" bind:checked={autoQuarantine24hEnabled} />{lang === 'zh'
				? '24 小时临时隔离'
				: '24h temporary quarantine'}</label
		>
		<label class="flex items-center gap-2"
			><input type="checkbox" bind:checked={autoQuarantine7dEnabled} />{lang === 'zh'
				? '7 天临时隔离'
				: '7d temporary quarantine'}</label
		>
	</div>
	<div class="mt-4 grid gap-3 sm:grid-cols-2">
		<label class="block"
			><span class="field-label"
				>{lang === 'zh' ? '每小时最多自动处置人数' : 'Maximum actions per hour'}</span
			><input
				class="input"
				type="number"
				min="1"
				max="100"
				bind:value={autoActionMaxPerHour}
			/></label
		>
		<label class="block"
			><span class="field-label"
				>{lang === 'zh' ? '最多占在线人数百分比' : 'Maximum percent of online players'}</span
			><input
				class="input"
				type="number"
				min="1"
				max="100"
				bind:value={autoActionMaxPercentOnline}
			/></label
		>
	</div>
	<button class="mt-4 btn btn-primary" disabled={busy} onclick={save}
		>{lang === 'zh' ? '保存自动处置设置' : 'Save automatic action settings'}</button
	>
</section>
<IntegritySettings
	orgId={data.orgId}
	config={data.ruleConfig}
	ruleDefaults={data.ruleDefaults}
	overrides={data.weaponOverrides}
	weaponDefaults={data.weaponDefaults}
	categories={data.weaponCategories}
	{lang}
/>
