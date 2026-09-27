<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import { pluginManifestSchema, type PluginManifest, type PersonalPlugin } from '$lib/plugins/sdk';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	const example = pluginManifestSchema.parse({
		apiVersion: 1,
		id: 'my-server-cards',
		name: '我的服务器卡片',
		widgets: [
			{ type: 'metric', title: '当前在线', metric: 'online' },
			{ type: 'metric', title: '平均延迟', metric: 'averagePing' },
			{
				type: 'players',
				title: '在线玩家击杀榜',
				columns: ['name', 'faction', 'kills', 'deaths'],
				limit: 10,
				sortBy: 'kills'
			}
		]
	});
	let editing = $state<string | null>(null);
	let manifestText = $state(JSON.stringify(example, null, 2));
	let name = $state(example.name);
	let description = $state(example.description);
	let accent = $state(example.style.accent);
	let columns = $state(example.style.columns);
	let density = $state<'comfortable' | 'compact'>(example.style.density);
	let serverId = $state('');
	let enabled = $state(true);
	let busy = $state(false);
	let message = $state('');
	let removing = $state<string | null>(null);
	function fill(manifest: PluginManifest, row?: PersonalPlugin) {
		editing = row?.manifest.id ?? null;
		manifestText = JSON.stringify(manifest, null, 2);
		name = manifest.name;
		description = manifest.description;
		accent = manifest.style.accent;
		columns = manifest.style.columns;
		density = manifest.style.density;
		serverId = row?.serverId ?? '';
		enabled = row?.enabled ?? true;
		message = '';
	}
	function parseText() {
		const result = pluginManifestSchema.safeParse(JSON.parse(manifestText));
		if (!result.success)
			throw new Error(
				result.error.issues.map((i) => `${i.path.join('.')}: ${i.message}`).join('；')
			);
		return result.data;
	}
	function applyJson() {
		try {
			const m = parseText();
			if (editing && m.id !== editing) throw new Error('编辑时请保留原插件标识。');
			const oldEditing = editing;
			fill(m, { manifest: m, serverId: serverId || null, enabled, updatedAt: '' });
			editing = oldEditing;
		} catch (err) {
			message = errorMessage(err);
		}
	}
	async function importFile(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		try {
			if (file.size > 65_536) throw new Error('文件不能超过 64 KB。');
			const raw = JSON.parse(await file.text());
			const m = pluginManifestSchema.parse(raw);
			fill(m);
		} catch (err) {
			message = errorMessage(err);
		} finally {
			input.value = '';
		}
	}
	function manifestForSave() {
		return pluginManifestSchema.parse({
			...parseText(),
			name,
			description,
			style: { accent, columns, density }
		});
	}
	async function save() {
		busy = true;
		message = '';
		try {
			const manifest = manifestForSave();
			await api(
				editing ? 'PUT' : 'POST',
				editing ? `/api/plugins/${encodeURIComponent(editing)}` : '/api/plugins',
				{ manifest, serverId: serverId || null, enabled }
			);
			await invalidateAll();
			editing = manifest.id;
			manifestText = JSON.stringify(manifest, null, 2);
			toast('插件已保存', 'ok');
		} catch (err) {
			message = errorMessage(err);
		} finally {
			busy = false;
		}
	}
	async function toggle(row: PersonalPlugin) {
		busy = true;
		try {
			await api('PUT', `/api/plugins/${encodeURIComponent(row.manifest.id)}`, {
				manifest: row.manifest,
				serverId: row.serverId,
				enabled: !row.enabled
			});
			await invalidateAll();
		} catch (err) {
			message = errorMessage(err);
		} finally {
			busy = false;
		}
	}
	async function remove(id: string) {
		busy = true;
		try {
			await api('DELETE', `/api/plugins/${encodeURIComponent(id)}`);
			await invalidateAll();
			removing = null;
			if (editing === id) fill(example);
		} catch (err) {
			message = errorMessage(err);
		} finally {
			busy = false;
		}
	}
	function download(manifest: PluginManifest) {
		const href = URL.createObjectURL(
			new Blob([JSON.stringify(manifest, null, 2)], { type: 'application/json' })
		);
		const a = document.createElement('a');
		a.href = href;
		a.download = `${manifest.id}.json`;
		a.click();
		setTimeout(() => URL.revokeObjectURL(href), 1000);
	}
</script>

<svelte:head><title>个人插件 · Warcon China</title></svelte:head>
<div class="mb-6 flex flex-wrap items-start justify-between gap-3">
	<div>
		<h1 class="text-2xl font-semibold text-white">个人插件</h1>
		<p class="mt-2 text-sm text-mist-400">
			创建自己的数据卡片和样式，或启用已安装的代码插件。配置仅属于当前账号。
		</p>
	</div>
	<a class="btn" href="/docs/plugins">开发文档与 API →</a>
</div>
{#if message}<p class="mb-4 panel p-3 text-warn" role="alert">{message}</p>{/if}
<section class="mb-6">
	<h2 class="mb-3 font-semibold text-white">我的插件（{data.plugins.length}/20）</h2>
	<div class="grid gap-3 lg:grid-cols-2">
		{#each data.plugins as row (row.manifest.id)}<article class="panel p-4">
				<div class="flex items-center justify-between gap-2">
					<h3 class="font-semibold text-white">{row.manifest.name}</h3>
					<span class="text-xs text-mist-400">{row.enabled ? '已启用' : '已停用'}</span>
				</div>
				<p class="mt-2 text-sm text-mist-400">{row.manifest.description}</p>
				<p class="mt-2 text-xs text-mist-400">
					{row.manifest.id} · v{row.manifest.version} · {row.manifest.renderer === 'cards'
						? 'JSON 卡片'
						: '代码组件'}
				</p>
				<p class="mt-1 text-xs text-mist-400">
					关联服务器：{data.pluginServers.find((s) => s.id === row.serverId)?.name ??
						(row.serverId ? '不可访问的服务器' : '未关联')}
				</p>
				<div class="mt-3 flex flex-wrap gap-2">
					<a class="btn" href={`/plugins/${encodeURIComponent(row.manifest.id)}`}>打开插件</a
					><button class="btn" disabled={busy} onclick={() => fill(row.manifest, row)}>编辑</button
					><button class="btn" disabled={busy} onclick={() => toggle(row)}
						>{row.enabled ? '停用' : '启用'}</button
					><button class="btn" onclick={() => download(row.manifest)}>导出 JSON</button><button
						class="btn"
						disabled={busy}
						onclick={() => (removing = row.manifest.id)}>删除</button
					>
				</div>
				{#if removing === row.manifest.id}<div class="mt-3 text-sm text-warn">
						删除此账号保存的插件配置？<button
							class="ml-2 btn"
							disabled={busy}
							onclick={() => remove(row.manifest.id)}>确认删除</button
						><button class="ml-2 btn" onclick={() => (removing = null)}>取消</button>
					</div>{/if}
			</article>{:else}<p class="text-sm text-mist-400">
				暂无个人插件。可从下方示例开始，或导入 JSON 文件。
			</p>{/each}
	</div>
</section>
<section class="panel p-5">
	<div class="flex flex-wrap items-center gap-3">
		<h2 class="font-semibold text-white">{editing ? '编辑插件' : '添加插件'}</h2>
		<button class="btn" disabled={busy} onclick={() => fill(example)}>新建 JSON 示例</button><label
			class="btn cursor-pointer"
			>导入 JSON<input
				class="hidden"
				type="file"
				accept=".json,application/json"
				onchange={importFile}
				disabled={busy}
			/></label
		><a class="btn" href="/examples/plugins/server-cards.json" download>下载 JSON 示例</a>
	</div>
	<div class="mt-4 flex flex-wrap gap-2">
		{#each data.catalog as manifest}<button
				class="btn"
				disabled={busy}
				onclick={() => fill(manifest)}>使用：{manifest.name}</button
			>{/each}
	</div>
	<form
		class="mt-5 space-y-4"
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<div class="grid gap-4 md:grid-cols-2">
			<label class="text-sm text-mist-300"
				>插件名称<input
					class="mt-1 input w-full"
					bind:value={name}
					maxlength="80"
					required
				/></label
			>
			<label class="text-sm text-mist-300"
				>关联服务器<select class="mt-1 input w-full" bind:value={serverId}
					><option value="">不关联（仅静态内容）</option>{#each data.pluginServers as server}<option
							value={server.id}>{server.orgName} / {server.name}</option
						>{/each}{#if serverId && !data.pluginServers.some((s) => s.id === serverId)}<option
							value={serverId}>原服务器已不可访问，请重新选择</option
						>{/if}</select
				></label
			>
			<label class="text-sm text-mist-300 md:col-span-2"
				>说明<input class="mt-1 input w-full" bind:value={description} maxlength="500" /></label
			>
			<label class="text-sm text-mist-300"
				>强调色<input class="mt-2 block h-10 w-20" type="color" bind:value={accent} /></label
			>
			<label class="text-sm text-mist-300"
				>卡片列数<select class="mt-1 input w-full" bind:value={columns}
					><option value={1}>1 列</option><option value={2}>2 列</option><option value={3}
						>3 列</option
					></select
				></label
			>
			<label class="text-sm text-mist-300"
				>卡片间距<select class="mt-1 input w-full" bind:value={density}
					><option value="comfortable">舒适</option><option value="compact">紧凑</option></select
				></label
			>
			<label class="flex items-center gap-2 text-sm text-mist-300"
				><input type="checkbox" bind:checked={enabled} />启用插件</label
			>
		</div>
		<label class="block text-sm text-mist-300"
			>插件 JSON（在 widgets 中增删数据卡片、玩家表格和文字）<textarea
				class="mt-2 min-h-80 input w-full font-mono text-xs"
				spellcheck="false"
				bind:value={manifestText}></textarea></label
		>
		<p class="text-xs text-mist-400">
			保存时以上方表单中的名称、说明和样式为准。修改 JSON 中的这些字段后，请先点击“应用 JSON
			到表单”。JSON 不包含服务器凭据。
		</p>
		<div class="flex flex-wrap gap-3">
			<button class="btn" type="button" onclick={applyJson} disabled={busy}>应用 JSON 到表单</button
			><button class="btn" type="submit" disabled={busy}>{busy ? '保存中…' : '保存插件'}</button>
		</div>
	</form>
</section>
