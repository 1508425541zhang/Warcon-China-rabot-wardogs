<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { api, errorMessage } from '$lib/api';
	import { pluginComponents } from '$lib/plugins/components';
	import type { PluginSnapshot } from '$lib/plugins/sdk';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	let snapshot = $state<PluginSnapshot | null>(null);
	let loading = $state(false);
	let message = $state<string | null>(null);
	let Component = $derived(pluginComponents[data.plugin.manifest.renderer]);
	let generation = 0;
	async function refresh() {
		if (loading || !data.plugin.enabled) return;
		const current = generation;
		loading = true;
		try {
			const result = await api<{ snapshot: PluginSnapshot | null }>(
				'GET',
				`/api/plugins/${encodeURIComponent(data.plugin.manifest.id)}/data`
			);
			if (current === generation) {
				snapshot = result.snapshot;
				message = null;
			}
		} catch (err) {
			if (current === generation) {
				snapshot = null;
				message = errorMessage(err);
			}
		} finally {
			if (current === generation) loading = false;
		}
	}
	// A keyed runner below is not sufficient for same-route navigation: reset requests and data too.
	$effect(() => {
		const id = data.plugin.manifest.id;
		const enabled = data.plugin.enabled;
		const revision = data.plugin.updatedAt;
		untrack(() => {
			generation++;
			snapshot = null;
			loading = false;
			message = null;
			if (id && enabled && revision) void refresh();
		});
	});
	onMount(() => {
		const timer = setInterval(() => {
			if (!document.hidden) void refresh();
		}, 10_000);
		return () => {
			generation++;
			clearInterval(timer);
		};
	});
</script>

<svelte:head><title>{data.plugin.manifest.name} · 个人插件</title></svelte:head>
<div class="mb-5 flex flex-wrap items-center justify-between gap-3">
	<div>
		<a href="/plugins" class="text-sm text-accent">← 个人插件</a>
		<h1 class="mt-2 text-2xl font-semibold text-white">{data.plugin.manifest.name}</h1>
		<p class="mt-2 text-sm text-mist-400">{data.plugin.manifest.description}</p>
	</div>
	<button class="btn" onclick={refresh} disabled={loading || !data.plugin.enabled}
		>{loading ? '读取中…' : '刷新数据'}</button
	>
</div>
{#if !data.plugin.enabled}<p class="panel p-5 text-warn">此插件已停用，请到个人插件页面启用。</p>
{:else}
	{#if message}<p class="mb-4 panel p-3 text-warn" role="alert">{message}</p>{/if}
	{#if !data.plugin.serverId}<p class="mb-4 text-sm text-mist-400">
			尚未关联服务器，静态文字仍可查看。请在个人插件页面编辑服务器设置。
		</p>{/if}
	{#if snapshot}<p class="mb-4 text-xs text-mist-400">
			玩家数据时间：{snapshot.playersAt
				? new Date(snapshot.playersAt).toLocaleString('zh-CN')
				: '尚未采集'} · 每 10 秒刷新
			{#if snapshot.stale}<strong class="text-warn"> · 数据已过期或尚未采集</strong>{/if}
			{#if snapshot.playersTruncated}<strong class="text-warn">
					· 玩家列表超过 256 人，列表已截取；合计指标不显示</strong
				>{/if}
		</p>{/if}
	{#key data.plugin.manifest.id}
		<svelte:boundary>
			{#if Component}<Component
					plugin={data.plugin.manifest}
					{snapshot}
					{loading}
					error={message}
				/>{:else}<p class="panel p-5 text-warn">
					该代码组件不在当前构建中，请联系部署管理员。
				</p>{/if}
			{#snippet failed(_error, reset)}<div class="panel p-5 text-warn">
					插件渲染失败。<button class="ml-3 btn" onclick={reset}>重试插件</button>
				</div>{/snippet}
		</svelte:boundary>
	{/key}
{/if}
