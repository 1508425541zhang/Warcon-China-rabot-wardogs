<script lang="ts">
	// Who holds a reserved slot on this server: the roster as the game server holds it, with what
	// the organisation's list and this server's own list contribute marked out, who is playing
	// right now, and the controls to hand out or withdraw a slot here. A slot reserved here goes
	// on the server's own list, so the panel applies it and lifts it at its expiry.
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, rconGet, rconPost, errorMessage } from '$lib/api';
	import { watchLive } from '$lib/live';
	import { fmtTime } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import SteamName from '$lib/components/SteamName.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import { isSteamId, steamProfiles, type SteamProfile } from '$lib/steam-profiles';
	import { describeSync, EXPIRY_OPTIONS, expiryIso, STATE_TONE } from '$lib/lists';
	import type { ListSyncServer, ServerListsState } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let admin = $derived(can(data.server.caps, 'slots.manage'));
	let listsEdit = $derived(can(data.server.caps, 'lists.reserve'));
	let orgPath = $derived(`/orgs/${encodeURIComponent(data.server.orgId)}`);
	// Builds without the live routes take reserved slots through the config document instead.
	let viaConfig = $derived(!data.features.reservedSlots && data.features.configDocument);
	let canReserve = $derived(admin && (data.features.reservedSlots || viaConfig));

	let listState = $state<ServerListsState | null>(null);
	$effect(() => {
		listState = data.listState;
	});
	/** SteamIDs the game server holds a slot for right now */
	let reserved = $state<string[]>([]);
	/**
	 * DefaultReservedPlayerIds as the config document has it, on builds where the panel edits the
	 * document (null otherwise). The live builds load it at start, so the two disagree between an
	 * edit and the next restart: shown as "arrives at restart" / "leaves at restart".
	 */
	let document = $state<string[] | null>(null);
	let reservedId = $state('');
	let newNote = $state('');
	let newExpiry = $state('0');
	let newCustom = $state('');
	let search = $state('');
	let busy = $state(false);
	/** who is on the server right now, by SteamID, with the name they are playing under */
	let online = $state<Record<string, string>>({});
	/** Steam personas for slot holders the panel has not seen play, where a key is configured */
	let steam = $state<Record<string, SteamProfile | null>>({});
	/** the persona for the id being typed into the reserve form: undefined while unknown */
	let preview = $state<SteamProfile | null | undefined>(undefined);
	let previewId = $derived(isSteamId(reservedId.trim()) ? reservedId.trim() : '');

	let orgReserveCount = $derived(
		data.orgLists?.lists.find((l) => l.kind === 'reserve')?.entryCount ?? null
	);
	let managedSlots = $derived(
		Object.values(listState?.reserved ?? {}).filter((s) => s.managed && s.scope === 'org').length
	);
	let hereSlots = $derived(
		Object.values(listState?.reserved ?? {}).filter((s) => s.managed && s.scope === 'server').length
	);
	let pendingCount = $derived(
		Object.values(listState?.reserved ?? {}).filter(
			(s) => s.state === 'pending' || s.state === 'failed'
		).length
	);
	/** MaxReservedSlots: player slots held back for reserved players; null until the worker read it */
	let heldSlots = $state<number | null>(null);
	/** the public cap the server reports (MaxPlayers less the held slots); null until seen */
	let publicSlots = $state<number | null>(null);
	let slotSource = (steamId: string) => listState?.reserved[steamId] ?? null;

	/**
	 * The roster: everyone holding a slot on this server, plus list entries still on their way.
	 * People playing right now come first, then the org's hand-picked entries, then this server's
	 * own, then slots added outside the panel, then members.
	 */
	let slots = $derived.by(() => {
		const ids = new Set([
			...reserved,
			...(document ?? []),
			...Object.keys(listState?.reserved ?? {})
		]);
		const rows = [...ids].flatMap((steamId) => {
			const src = slotSource(steamId);
			const here = reserved.includes(steamId);
			const inDocument = document?.includes(steamId) ?? here;
			// A slot no org list manages that the server no longer reports (nor the document) is a
			// stale copy, withdrawn from the official console or here a moment ago: nothing to show.
			if (src && !src.managed && !here && !inDocument) return [];
			return {
				steamId,
				src,
				here,
				/** the document and the running server disagree until the server restarts */
				pending: here && !inDocument ? 'leaves' : !here && inDocument ? 'arrives' : null,
				name: online[steamId] ?? src?.name ?? steam[steamId]?.name ?? null,
				online: steamId in online,
				rank: src?.member ? 4 : !src?.managed ? 3 : src.scope === 'server' ? 2 : 1
			};
		});
		return rows.sort(
			(a, b) =>
				Number(b.online) - Number(a.online) ||
				a.rank - b.rank ||
				(a.name ?? '￿').localeCompare(b.name ?? '￿') ||
				a.steamId.localeCompare(b.steamId)
		);
	});
	/** a column sort on top of that order; clicking the active header a third time restores it */
	const sort = new TableSort<(typeof slots)[number]>({
		player: { by: (s) => s.name },
		steamId: { by: (s) => s.steamId },
		source: { by: (s) => s.rank },
		note: { by: (s) => s.src?.note },
		expires: { by: (s) => s.src?.expiresAt ?? '' }
	});
	let rows = $derived(
		sort.sorted(slots.filter((s) => matches(search, s.steamId, s.name, s.src?.note)))
	);
	let onlineSlots = $derived(slots.filter((s) => s.online).length);
	let localSlots = $derived(slots.filter((s) => s.here && !s.src?.managed).length);

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
			toast(result?.message || '操作已完成。', 'ok');
			if (opts.after) await opts.after();
			return result;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return null;
		}
	}
	async function refreshListState() {
		try {
			listState = await api<ServerListsState>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/lists/state`
			);
		} catch (err) {
			console.warn('列表状态', err);
		}
	}
	async function refreshReserved() {
		const r = await rconGet<{ reserved: string[]; document?: string[] | null }>(
			id,
			'reserved',
			viaConfig ? { document: 1 } : undefined
		);
		reserved = r.reserved;
		document = viaConfig ? (r.document ?? null) : null;
		void refreshListState();
	}
	const refreshAll = () => Promise.all([refreshReserved(), invalidateAll()]);

	$effect(() => {
		void id;
		refreshReserved().catch((err) => toast(errorMessage(err), 'err'));
	});
	// Personas are looked up for the roster's ids alone, so a lookup never re-runs on its own result.
	let slotIds = $derived(
		[...new Set([...reserved, ...(document ?? []), ...Object.keys(listState?.reserved ?? {})])]
			.sort()
			.join(',')
	);
	$effect(() => {
		const ids = slotIds.split(',').filter(Boolean);
		untrack(() => void lookupSteam(ids));
	});
	async function lookupSteam(ids: string[]) {
		const found = await steamProfiles(ids.filter((s) => !(s in steam)));
		if (Object.keys(found).length) steam = { ...steam, ...found };
	}
	$effect(() => {
		const want = previewId;
		preview = undefined;
		if (!want) return;
		void steamProfiles([want]).then((r) => {
			if (previewId === want && want in r) preview = r[want];
		});
	});
	$effect(() => {
		void id;
		return watchLive([id], (v) => {
			const next: Record<string, string> = {};
			for (const p of v.players) next[p.steamId] = p.name;
			online = next;
			heldSlots = v.reservedSlots;
			publicSlots = v.status?.maxPlayers ?? null;
		});
	});

	async function syncNow() {
		busy = true;
		try {
			const res = await api<{ sync: ListSyncServer }>(
				'POST',
				`/api/servers/${encodeURIComponent(id)}/lists/sync`
			);
			toast(describeSync({ servers: [res.sync] }, '同步已执行。'), 'ok', 8000);
			await refreshAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	/** Reserves through the server's own list; the panel applies it now and lifts it at the expiry. */
	async function addSlot() {
		const steamId = reservedId.trim();
		busy = true;
		try {
			const res = await api<{ sync: ListSyncServer }>(
				'POST',
				`/api/servers/${encodeURIComponent(id)}/lists/reserve/entries`,
				{ steamId, reason: newNote.trim(), expiresAt: expiryIso(newExpiry, newCustom) }
			);
			toast(describeSync({ servers: [res.sync] }, `已为 ${steamId} 分配预留席位。`), 'ok', 8000);
			reservedId = '';
			newNote = '';
			newExpiry = '0';
			newCustom = '';
			await refreshReserved();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function removeSlot(steamId: string, name: string | null) {
		const src = slotSource(steamId);
		const who = name ? `${name} (${steamId})` : steamId;
		if (src?.managed && src.scope === 'server') {
			if (!(await confirmDialog(`确定撤回 ${who} 的预留席位？`, { okLabel: '撤回' }))) return;
			busy = true;
			try {
				const res = await api<{ sync: ListSyncServer }>(
					'DELETE',
					`/api/servers/${encodeURIComponent(id)}/lists/reserve/entries/${steamId}`
				);
				toast(describeSync({ servers: [res.sync] }, `已撤回 ${who} 的预留席位。`), 'ok', 8000);
				await refreshReserved();
			} catch (err) {
				toast(errorMessage(err), 'err');
			} finally {
				busy = false;
			}
			return;
		}
		await act(
			'reservedRemove',
			{ steamId, viaConfig },
			{
				confirm: src?.managed
					? `${who} holds this slot through the organisation's list, so the panel will hand it back at the next sync. Withdraw it here anyway? To withdraw it everywhere, remove it from the organisation's reserved slots instead.`
					: `Withdraw the reserved slot for ${who}?`,
				danger: !!src?.managed,
				after: refreshReserved
			}
		);
	}
</script>

<div class="mb-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
	<div class="panel">
		<span class="label-sm">已预留位置</span>
		<div class="flex items-baseline gap-2">
			<span class="font-display text-[34px] leading-none font-semibold tabular"
				>{reserved.length}</span
			>
			{#if onlineSlots}
				<span class="ml-auto inline-flex items-center gap-1.5 text-[12.5px] text-ok"
					><span class="size-1.5 rounded-full bg-ok"></span>{onlineSlots} 当前在线</span
				>
			{/if}
		</div>
		{#if heldSlots !== null}
			<div class="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
				<span
					><span class="text-mist-400">玩家位</span>
					<b>{publicSlots ?? '—'}</b> 公开 + <b>{heldSlots}</b> 预留{#if publicSlots !== null}
						= {publicSlots + heldSlots}{/if}</span
				>
			</div>
		{/if}
		<p class="note">
			名单中的玩家可以跳过加入队列，名单本身没有长度上限。MaxReservedSlots
			只决定为他们保留多少个玩家位。
		</p>
	</div>

	<div class="panel">
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<span class="label-sm mb-0!">来自组织 · {data.server.orgName}</span>
			<span class="ml-auto inline-flex flex-wrap gap-1.5">
				{#if listState?.canEditOrgSlots}
					<a class="btn btn-sm" href="{orgPath}/reserved">组织名单</a>
				{/if}
				{#if listsEdit}
					<button class="btn btn-sm" disabled={busy} onclick={syncNow}>立即同步</button>
				{/if}
			</span>
		</div>
		<div class="flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
			{#if orgReserveCount !== null}
				<span><b>{orgReserveCount}</b> 在组织名单中， <b>{managedSlots}</b> 已在本服应用</span>
			{:else}
				<span><b>{managedSlots}</b> 由组织在本服应用</span>
			{/if}
			<span><b>{hereSlots}</b> 在本服预留</span>
			<span><b>{localSlots}</b> 在面板外添加</span>
			{#if pendingCount}<Badge tone="warn">{pendingCount} 等待中或失败</Badge>{/if}
		</div>
		<div class="mt-1 text-[12.5px] text-mist-400">
			{#if listState?.sync?.syncedAt}
				上次同步 {fmtTime(listState.sync.syncedAt)}.
			{:else}
				尚未同步。
			{/if}
			{#if listState?.sync?.lastError}<span class="text-danger">
					{listState.sync.lastError}</span
				>{/if}
		</div>
	</div>

	<div class="flex flex-col panel">
		<span class="label-sm">在本服预留位置</span>
		<form
			class="space-y-2"
			onsubmit={(e) => {
				e.preventDefault();
				void addSlot();
			}}
		>
			<div class="join w-full">
				<input
					class="input font-mono"
					type="text"
					inputmode="numeric"
					placeholder="SteamID64…"
					maxlength="17"
					required
					disabled={!canReserve}
					bind:value={reservedId}
				/>
				<button
					type="submit"
					class="btn btn-primary"
					disabled={!canReserve || busy || !isSteamId(reservedId.trim())}>预留</button
				>
			</div>
			{#if previewId && preview}
				<div class="text-[12.5px]"><SteamName profile={preview} /></div>
			{:else if previewId && preview === null}
				<div class="text-[12.5px] text-mist-600">找不到该 SteamID 的资料。</div>
			{/if}
			<input
				class="input"
				type="text"
				maxlength="200"
				placeholder="备注，例如捐赠者、战队成员（可选）"
				disabled={!canReserve}
				bind:value={newNote}
			/>
			<div class="flex flex-wrap gap-2">
				<label class="block sm:w-40"
					><span class="field-label">到期时间</span><select
						class="input"
						disabled={!canReserve}
						bind:value={newExpiry}
					>
						{#each EXPIRY_OPTIONS as [value, label] (value)}
							<option {value}>{label}</option>
						{/each}
					</select></label
				>
				{#if newExpiry === 'custom'}
					<label class="block sm:flex-1"
						><span class="field-label">截止时间（本地）</span><input
							class="input"
							type="datetime-local"
							bind:value={newCustom}
							required
						/></label
					>
				{/if}
			</div>
		</form>
		<p class="note">
			{#if !data.features.reservedSlots && !viaConfig}
				当前服务器版本既没有实时预留位接口，也没有可写的配置文件，因此无法在此分配预留位。
			{:else}
				仅在本服生效；设有到期时间的预留位会由面板届时撤回。
				{#if viaConfig}
					此版本没有实时预留位接口，因此面板会把预留位写入配置文件的
					+DefaultReservedPlayerIds，并在下次重启时生效。
				{/if}
				{#if listState?.canEditOrgSlots}要在所有服务器分配预留位，请使用组织名单。{/if}
			{/if}
		</p>
	</div>
</div>

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0!">预留位玩家</span>
		<div class="join w-full sm:ml-auto sm:w-auto sm:min-w-[320px]">
			<input
				class="input"
				type="search"
				placeholder="按名称、SteamID 或备注筛选…"
				bind:value={search}
			/>
			<button
				class="btn"
				onclick={() => refreshReserved().catch((e) => toast(errorMessage(e), 'err'))}>刷新</button
			>
		</div>
	</div>
	{#if slots.length}
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<SortHeader {sort} key="player">玩家</SortHeader>
						<SortHeader {sort} key="steamId">SteamID64</SortHeader>
						<SortHeader {sort} key="source">来源</SortHeader>
						<SortHeader {sort} key="note">备注</SortHeader>
						<SortHeader {sort} key="expires">到期时间</SortHeader>
						<th></th>
					</tr>
				</thead>
				<tbody>
					{#each rows as s (s.steamId)}
						<tr class={s.src?.managed && !s.here ? 'opacity-70' : ''}>
							<td>
								<span class="inline-flex min-w-0 items-center gap-2.5">
									<span
										class="size-2 shrink-0 rounded-full {s.online
											? 'bg-ok ring-[3px] ring-ok/25'
											: 'bg-ink-700'}"
										title={s.online ? '当前在线' : '当前不在服务器上'}
									></span>
									{#if steam[s.steamId]?.avatar}<img
											src={steam[s.steamId]?.avatar}
											alt=""
											class="size-5 shrink-0 rounded-sm"
											loading="lazy"
											referrerpolicy="no-referrer"
										/>{/if}
									<a
										href="/server/{encodeURIComponent(id)}/players/{s.steamId}"
										class="truncate font-medium hover:text-accent hover:underline {s.name
											? ''
											: 'text-mist-400 italic'}"
										title="打开玩家档案">{s.name ?? '尚未在此观测到'}</a
									>
									{#if s.online}<span class="caps text-[10px] text-ok">游玩中</span>{/if}
								</span>
							</td>
							<td class="font-mono text-[12.5px] text-mist-400">{s.steamId}</td>
							<td>
								{#if s.src?.member}
									<Badge tone="accent">成员</Badge>
								{:else if s.src?.managed}
									<Badge tone={STATE_TONE[s.src.state]}
										>{s.src.scope === 'server' ? 'here' : 'org'}{s.src.state === 'applied'
											? ''
											: ` · ${s.src.state}`}</Badge
									>
								{:else}
									<Badge>本服</Badge>
								{/if}
								{#if s.pending === 'leaves'}
									<Badge tone="warn" class="ml-1" title="已从配置文件移除，服务器重启前仍会保留"
										>重启后移除</Badge
									>
								{:else if s.pending === 'arrives'}
									<Badge tone="warn" class="ml-1" title="写入配置文件，服务器重启后生效"
										>重启后生效</Badge
									>
								{/if}
							</td>
							<td
								>{#if s.src?.note}{s.src.note}{:else}<span class="text-mist-600">—</span>{/if}</td
							>
							<td class="whitespace-nowrap text-mist-400"
								>{#if s.src?.expiresAt}{fmtTime(s.src.expiresAt)}{:else}<span class="text-mist-600"
										>—</span
									>{/if}</td
							>
							<td class="text-right">
								{#if canReserve && ((s.src?.managed && s.src.scope === 'server') || ((s.here || s.pending === 'arrives') && s.pending !== 'leaves'))}
									<button
										type="button"
										class="btn btn-sm btn-ghost"
										title="撤回此预留位"
										onclick={() => removeSlot(s.steamId, s.name)}>撤回</button
									>
								{/if}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-6 text-center text-mist-600">没有符合筛选条件的玩家。</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="note">
			<Badge tone="ok">组织</Badge> 和 <Badge tone="accent">成员</Badge> 个预留位来自组织；若在这里撤回，会交还组织名单处理；
			<Badge tone="ok">此处</Badge> 个预留位由面板在本服分配，到期时自动撤回； <Badge>本服</Badge> 个预留位由面板外部添加，面板不会修改。{#if viaConfig}
				此版本在启动时读取配置文件中的预留名单，因此这里的分配或撤回会标记为待重启生效。{/if}
		</p>
	{:else}
		<div class="callout mb-0">
			<b>本服尚无人持有预留位。</b>
			<span class="block text-mist-400"
				>预留位让管理员、捐赠者和战队成员在服务器满员时跳过队列。你可以在上方为本服分配，{#if listState?.canEditOrgSlots}也可以通过组织名单在所有服务器分配。{/if}.</span
			>
		</div>
	{/if}
</div>
