<script lang="ts">
	import { page } from '$app/state';
	import { goto, invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();
	let owner = $derived(data.listsRole.owner);
	let base = $derived(`/orgs/${encodeURIComponent(data.org.id)}`);
	let orgPath = $derived(`/api/orgs/${encodeURIComponent(data.org.id)}`);
	let current = $derived(page.url.pathname.slice(base.length) || '');

	/** path, label, and who sees it: owners only, anyone who opens a list, or one list's editors */
	const TABS = [
		['', 'Overview', 'owner'],
		['/servers', 'Servers', 'any'],
		['/access', 'Access', 'owner'],
		['/roles', 'Roles', 'owner'],
		['/players', 'Players', 'any'],
		['/bans', '封禁列表', 'ban'],
		['/reserved', '预留席位', 'reserve']
	] as const;
	let tabs = $derived(
		TABS.filter(
			([, , who]) => who === 'any' || (who === 'owner' ? owner : data.listsRole.kinds.includes(who))
		)
	);

	let renaming = $state<string | null>(null);
	let busy = $state(false);

	async function rename() {
		if (renaming === null) return;
		busy = true;
		try {
			await api('PATCH', orgPath, { name: renaming.trim() });
			toast('组织已重命名。', 'ok');
			renaming = null;
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function deleteOrg() {
		const n = data.orgServers.length;
		if (
			!(await confirmDialog(
				`确定删除“${data.org.name}”？这会从面板移除其 ${n} 台服务器、所有成员关系和邀请链接。审计历史会保留。`,
				{ okLabel: '删除组织', danger: true }
			))
		)
			return;
		busy = true;
		try {
			await api('DELETE', orgPath);
			toast('组织已删除。', 'ok');
			await invalidateAll();
			await goto('/orgs');
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{data.org.name} · {data.appName}</title></svelte:head>

<div class="mb-4 flex flex-wrap items-center gap-3">
	<div>
		<a href="/orgs" class="caps text-mist-400 hover:text-mist-100">组织</a>
		<h1 class="text-xl font-semibold tracking-tight">{data.org.name}</h1>
	</div>
	{#if owner}
		<span class="ml-auto inline-flex gap-1.5">
			<button class="btn btn-sm" onclick={() => (renaming = data.org.name)}>重命名</button>
			<button class="btn btn-sm btn-danger" onclick={deleteOrg} disabled={busy}>删除</button>
		</span>
	{/if}
</div>

<nav class="strip mb-5 gap-1 border-b border-white/8 pb-3" aria-label="组织栏目">
	{#each tabs as [path, label] (path)}
		<a href="{base}{path}" class="tab-link {current === path ? 'tab-link-active' : ''}">{label}</a>
	{/each}
</nav>

{#if data.org.suspended}
	<div class="callout border-l-danger">
		<b>已暂停</b> 自 {fmtTime(data.org.suspended.at)}{#if data.org.suspended.reason}: {data.org
				.suspended.reason}{/if}。在平台所有者恢复组织前，成员无法访问其服务器，邀请链接也无法使用。
	</div>
{/if}

{#key data.org.id}
	{@render children()}
{/key}

{#if renaming !== null}
	<Modal title="重命名组织" onclose={() => (renaming = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				void rename();
			}}
		>
			<label class="block"
				><span class="field-label">名称</span><input
					class="input"
					type="text"
					bind:value={renaming}
					minlength="2"
					maxlength="60"
					required
				/></label
			>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (renaming = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}>保存</button>
			</div>
		</form>
	</Modal>
{/if}
