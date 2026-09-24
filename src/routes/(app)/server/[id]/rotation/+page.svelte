<script lang="ts">
	import { rconGet, rconPost, errorMessage } from '$lib/api';
	import { expSetLabel, lightingLabel, mapLabel, zoneLabel } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { rotationFromText, rotationIntoText, type RotationDoc } from '$lib/rotation-doc';
	import Badge from '$lib/components/Badge.svelte';
	import MapPicker from '$lib/components/MapPicker.svelte';
	import MapArt from '$lib/components/MapArt.svelte';
	import type { ConfigDoc, ConfigResult, MapSelection, Rotation, RotationEntry } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let rotationEdit = $derived(can(data.server.caps, 'rotation.edit'));
	let matchControl = $derived(can(data.server.caps, 'match.control'));
	let rotationSave = $derived(can(data.server.caps, 'rotation.save'));
	let configApply = $derived(can(data.server.caps, 'config.apply'));
	let configHref = $derived(`/server/${encodeURIComponent(id)}/config`);

	// Live build CL-499480 serves none of the rotation edit routes. On such a build this tab edits
	// the rotation section of the config document instead: the same table and buttons, staged
	// locally and written to the server in one apply. The server rebuilds its rotation at once and
	// uses the new order from the next map change. The document is for those who may apply it;
	// everyone else reads the live rotation, which such a build cannot edit.
	let viaDoc = $derived(!data.features.rotationEdit && configApply);
	let liveToggle = $derived(!viaDoc && data.features.liveSettings);

	let rotation = $state<Rotation | null>(null);
	let selected = $state(-1);
	let picker = $state<MapPicker>();

	// ---- document mode state ----
	let doc = $state<ConfigDoc | null>(null);
	let docError = $state('');
	let base = $state<RotationDoc>({ enabled: true, mode: 'ordered', entries: [] });
	let staged = $state<RotationDoc>({ enabled: true, mode: 'ordered', entries: [] });
	let busy = $state(false);
	const same = (a: RotationDoc, b: RotationDoc) => JSON.stringify(a) === JSON.stringify(b);
	let dirty = $derived(viaDoc && !same(staged, base));
	let canApply = $derived(configApply && !!doc?.writable);
	let canEdit = $derived(viaDoc ? canApply : rotationEdit && data.features.rotationEdit);
	let canToggle = $derived(viaDoc ? canApply : rotationSave && data.features.liveSettings);

	const clone = (r: RotationDoc): RotationDoc => JSON.parse(JSON.stringify(r));

	async function act(
		action: string,
		params: object,
		opts: { confirm?: string; danger?: boolean; after?: () => Promise<unknown> } = {}
	) {
		if (
			opts.confirm &&
			!(await confirmDialog(opts.confirm, { okLabel: '确认执行', danger: opts.danger }))
		)
			return null;
		try {
			const result = await rconPost<{ message?: string }>(id, action, params);
			toast(result?.message || `${action} done.`, 'ok');
			if (opts.after) await opts.after();
			return result;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return null;
		}
	}
	async function refresh() {
		try {
			rotation = await rconGet<Rotation>(id, 'rotation');
		} catch (err) {
			toast(errorMessage(err), 'err');
		}
	}
	async function loadDoc() {
		try {
			doc = await rconGet<ConfigDoc>(id, 'config');
			docError = '';
			base = rotationFromText(doc.text);
			staged = clone(base);
		} catch (err) {
			doc = null;
			docError = errorMessage(err);
		}
	}
	$effect(() => {
		void refresh();
		if (viaDoc) void loadDoc();
	});

	// The rows shown: the live rotation, or the staged document. Now/next badges come from the
	// live rotation and only make sense while the staged list still matches it.
	let rows = $derived.by((): RotationEntry[] => {
		if (!viaDoc) return rotation?.entries ?? [];
		return staged.entries.map((e, i) => ({
			...e,
			denied: false,
			status: !dirty && rotation?.entries[i] ? rotation.entries[i].status : ''
		}));
	});
	let nowIndex = $derived(
		viaDoc ? (dirty ? -1 : (rotation?.nowIndex ?? -1)) : (rotation?.nowIndex ?? -1)
	);
	let nextIndex = $derived(
		viaDoc ? (dirty ? -1 : (rotation?.nextIndex ?? -1)) : (rotation?.nextIndex ?? -1)
	);
	let enabledShown = $derived(viaDoc ? staged.enabled : !!rotation?.enabled);
	let modeShown = $derived(
		viaDoc ? staged.mode : rotation?.mode === 'random' ? 'random' : 'ordered'
	);

	const entryToSelection = (e: RotationEntry): MapSelection => ({
		map: e.map,
		experiences: e.experiences,
		lighting: e.lighting,
		zoneAlternator: e.zoneAlternator
	});
	function withSel(fn: (i: number) => unknown) {
		if (selected < 0 || !rows[selected]) {
			toast('请先选择一个地图轮换条目。', 'err');
			return;
		}
		return fn(selected);
	}

	// ---- edits: live routes, or the staged document ----
	function move(i: number, direction: 'up' | 'down') {
		if (viaDoc) {
			const j = direction === 'up' ? i - 1 : i + 1;
			if (j < 0 || j >= staged.entries.length) return;
			const list = staged.entries.slice();
			[list[i], list[j]] = [list[j], list[i]];
			staged.entries = list;
			selected = j;
			return;
		}
		void act(
			'rotationMove',
			{ index: i, direction },
			{
				after: async () => {
					selected = direction === 'up' ? Math.max(0, i - 1) : i + 1;
					await refresh();
				}
			}
		);
	}
	async function remove(i: number) {
		if (viaDoc) {
			staged.entries = staged.entries.filter((_, k) => k !== i);
			selected = -1;
			return;
		}
		await act(
			'rotationRemove',
			{ index: i },
			{
				confirm: `Remove rotation entry ${i + 1}?`,
				danger: true,
				after: async () => {
					selected = -1;
					await refresh();
				}
			}
		);
	}
	function add() {
		if (!picker) return;
		const sel = picker.selection();
		if (viaDoc) {
			staged.entries = [...staged.entries, sel];
			selected = staged.entries.length - 1;
			return;
		}
		void act('rotationAdd', sel, { after: refresh });
	}
	function setEnabled(on: boolean) {
		if (viaDoc) {
			staged.enabled = on;
			return;
		}
		void act('rotationSettings', { rotationEnabled: on }, { after: refresh });
	}
	function setMode(mode: string) {
		if (viaDoc) {
			staged.mode = mode === 'random' ? 'random' : 'ordered';
			return;
		}
		void act('rotationSettings', { rotationMode: mode }, { after: refresh });
	}
	function discard() {
		staged = clone(base);
		selected = -1;
	}
	async function applyDoc() {
		if (!doc || !dirty) return;
		busy = true;
		try {
			const text = rotationIntoText(doc.text, staged);
			const r = await rconPost<ConfigResult>(id, 'configApply', { text, revision: doc.revision });
			if (r.conflict) {
				toast(
					`${r.errorMessage || '加载后服务器上的配置已发生变化。'} 已重新加载；你的修改已保留，请再次点击“应用”。`,
					'err'
				);
				const keep = clone(staged);
				await loadDoc();
				staged = keep;
				return;
			}
			if (!r.ok) {
				toast(r.errors?.[0]?.message || r.errorMessage || 'Rejected.', 'err');
				return;
			}
			toast(
				`Rotation applied (revision ${r.revision}). The server rebuilt it; the new order is used from the next map change.`,
				'ok'
			);
			await Promise.all([loadDoc(), refresh()]);
			selected = -1;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	// Long rotations scroll inside the table, so keep the row being worked on in view.
	let tableWrap = $state<HTMLDivElement>();
	$effect(() => {
		const row = tableWrap?.querySelector(selected >= 0 ? 'tr.selected' : 'tr.now');
		row?.scrollIntoView({ block: 'nearest' });
	});
</script>

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-3">
		<span class="mr-auto label-sm mb-0">地图轮换</span>
		{#if rotation || doc}
			<label class="inline-flex items-center gap-2 text-[13px]">
				<input
					type="checkbox"
					checked={enabledShown}
					disabled={!canToggle}
					onchange={(e) => setEnabled(e.currentTarget.checked)}
				/> 已启用
			</label>
			<select
				class="input w-32"
				value={modeShown}
				disabled={!canToggle}
				onchange={(e) => setMode(e.currentTarget.value)}
			>
				<option value="ordered">已排序</option>
				<option value="random">随机</option>
			</select>
		{/if}
	</div>
	{#if viaDoc}
		<div class="callout">
			此服务器版本不支持实时编辑地图轮换。这里的修改会先暂存，然后一次性写入
			<a class="link" href={configHref}>配置文件</a>
			。服务器会立即重建轮换列表，并从下次切图起使用新顺序；如果之后仍按旧顺序运行，请重启服务器以重新读取配置文件。
			{#if docError}<br
				/>无法读取配置文件（{docError}），因此无法在此修改。{:else if doc && !doc.writable}<br
				/>此服务器的配置文件为只读，无法在此修改。{/if}
		</div>
	{:else if !liveToggle}
		<div class="callout">
			此版本无法实时开启、关闭地图轮换或修改轮换模式。请在 <a class="link" href={configHref}
				>配置文件</a
			> 中设置 bEnabled 和 RotationMode。
		</div>
	{/if}
	<div class="mb-3 flex flex-wrap items-end gap-x-4 gap-y-3">
		<div class="field-group">
			<span class="field-label">选中条目</span>
			<div class="join join-stack w-full">
				<button class="btn" disabled={!canEdit} onclick={() => withSel((i) => move(i, 'up'))}
					>上移</button
				>
				<button class="btn" disabled={!canEdit} onclick={() => withSel((i) => move(i, 'down'))}
					>下移</button
				>
				{#if !viaDoc}
					<button
						class="btn"
						disabled={!matchControl}
						onclick={() =>
							withSel((i) => act('setNextMap', entryToSelection(rows[i]), { after: refresh }))}
						>设为下一张地图</button
					>
				{/if}
				<button class="btn btn-danger" disabled={!canEdit} onclick={() => withSel(remove)}
					>移除</button
				>
			</div>
		</div>
		{#if viaDoc}
			<div class="join join-stack w-full sm:ml-auto sm:w-auto">
				<button class="btn" disabled={!dirty || busy} onclick={discard}>放弃</button>
				<button class="btn btn-primary" disabled={!dirty || !canApply || busy} onclick={applyDoc}
					>应用到服务器{#if dirty}
						&nbsp;（{staged.entries.length} 项）{/if}</button
				>
			</div>
		{:else}
			<button
				class="btn w-full btn-primary sm:ml-auto sm:w-auto"
				disabled={!rotationSave || !data.features.rotationSave}
				onclick={() => act('rotationSave', {})}>保存地图轮换</button
			>
		{/if}
	</div>
	<div class="max-h-[55vh] table-wrap overflow-y-auto" bind:this={tableWrap}>
		<table>
			<thead
				><tr
					><th class="num">#</th><th>地图</th><th>游戏模式与模组</th><th>时间与天气</th><th
						>控制区</th
					></tr
				></thead
			>
			<tbody>
				{#each rows as e, i (i)}
					<tr
						class="clickable {selected === i ? 'selected' : ''} {i === nowIndex ? 'now' : ''}"
						onclick={() => (selected = selected === i ? -1 : i)}
					>
						<td class="num">{i + 1}</td>
						<td>
							<span class="inline-flex items-center gap-2.5">
								<MapArt
									map={e.map}
									lighting={e.lighting}
									variant="720"
									alt=""
									class="w-14 shrink-0"
								/>
								{mapLabel(data.catalog, e.map)}
							</span>
							{#if i === nowIndex}<Badge tone="accent" class="ml-1">当前</Badge
								>{:else if i === nextIndex}<Badge tone="info" class="ml-1">下一张</Badge>{/if}
							{#if e.denied}<Badge tone="err" class="ml-1">已拒绝</Badge>{/if}
						</td>
						<td>{expSetLabel(data.catalog, e.experiences)}</td>
						<td>{lightingLabel(data.catalog, e.lighting)}</td>
						<td>{zoneLabel(e.zoneAlternator)}</td>
					</tr>
				{:else}
					<tr
						><td colspan="5" class="py-6 text-center text-mist-600"
							>{rotation || doc ? '地图轮换列表为空。' : '加载中…'}</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="note">
		{#if viaDoc}
			{#if dirty}未应用的修改：已暂存 {staged.entries.length} 项，服务器当前有 {base.entries.length} 项。点击“应用”前不会发送。{:else}与配置文件一致（版本
				{doc?.revision || '—'}）。{/if}
		{:else}
			修改会立即应用到正在运行的地图轮换。点击“保存轮换”可写入配置文件，使重启后仍保留；服务器需以
			-StandaloneConfig 启动。
		{/if}
	</p>
</div>

<div class="mt-4 panel">
	<span class="label-sm">添加轮换地图</span>
	<MapPicker bind:this={picker} serverId={id} catalog={data.catalog} disabled={!canEdit} />
	<div class="mt-4">
		<button class="btn btn-primary" disabled={!canEdit} onclick={add}>加入地图轮换</button>
	</div>
</div>
