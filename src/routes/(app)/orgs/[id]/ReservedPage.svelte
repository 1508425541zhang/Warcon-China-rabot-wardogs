<script lang="ts">
	// The organisation's reserved-slot list, laid out like a server's Reserved slots tab: how many
	// hold a slot and who is playing right now, where the list stands on each server, the form to
	// hand out a slot everywhere, and the roster itself.
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { watchLive } from '$lib/live';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { describeSync, EXPIRY_OPTIONS, expiryIso, STATE_TEXT, STATE_TONE } from '$lib/lists';
	import { isSteamId, steamProfiles, type SteamProfile } from '$lib/steam-profiles';
	import Badge from '$lib/components/Badge.svelte';
	import SteamName from '$lib/components/SteamName.svelte';
	import ImportCandidates from './ImportCandidates.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { ListEntryView, ListSyncSummary, OrgListsView } from '$lib/types';

	let {
		entries,
		lists,
		org,
		servers
	}: {
		entries: ListEntryView[];
		lists: OrgListsView;
		org: { id: string; name: string };
		/** the org's servers this user may open: who is playing is watched on these */
		servers: { id: string; name: string }[];
	} = $props();

	let path = $derived(`/api/orgs/${encodeURIComponent(org.id)}/lists/reserve/entries`);
	let owner = $derived(lists.role === 'owner');
	let search = $state('');
	let busy = $state(false);
	let newId = $state('');
	let newReason = $state('');
	let newExpiry = $state('0');
	let newCustom = $state('');
	/** Steam personas, for the avatar beside a name */
	let steam = $state<Record<string, SteamProfile | null>>({});
	/** the persona for the id being typed into the reserve form: undefined while unknown */
	let preview = $state<SteamProfile | null | undefined>(undefined);
	let previewId = $derived(isSteamId(newId.trim()) ? newId.trim() : '');
	/** the worker's latest player list per server, by server id */
	let playersOn = $state<Record<string, { steamId: string; name: string }[]>>({});
	/** who is playing right now, by SteamID: the name they play under and where */
	let online = $derived.by(() => {
		const out: Record<string, { name: string; servers: string[] }> = {};
		for (const s of servers)
			for (const p of playersOn[s.id] ?? [])
				(out[p.steamId] ??= { name: p.name, servers: [] }).servers.push(s.name);
		return out;
	});

	let picked = $derived(entries.filter((e) => !e.member).length);
	let members = $derived(entries.length - picked);
	let playing = $derived(entries.filter((e) => e.steamId in online).length);
	/** where the list stands on each server: how many entries the panel has applied there */
	let standing = $derived(
		lists.servers.map((s) => {
			const states = entries.map((e) => e.servers.find((x) => x.serverId === s.id)?.state);
			return {
				...s,
				applied: states.filter((st) => st === 'applied' || st === 'local').length,
				pending: states.filter((st) => st === 'pending' || st === 'failed').length,
				href: servers.some((x) => x.id === s.id)
					? `/server/${encodeURIComponent(s.id)}/slots`
					: null
			};
		})
	);
	let dossierBase = $derived(
		servers.length ? `/server/${encodeURIComponent(servers[0].id)}/players` : null
	);

	/**
	 * The roster: people playing right now first, then slots handed out by hand, then members.
	 */
	let roster = $derived(
		entries
			.map((e) => {
				const live = online[e.steamId];
				return {
					e,
					live,
					name: live?.name ?? e.name ?? steam[e.steamId]?.name ?? null,
					avatar: steam[e.steamId]?.avatar ?? ''
				};
			})
			.sort(
				(a, b) =>
					Number(!!b.live) - Number(!!a.live) ||
					Number(a.e.member) - Number(b.e.member) ||
					(a.name ?? '￿').localeCompare(b.name ?? '￿') ||
					a.e.steamId.localeCompare(b.e.steamId)
			)
	);
	/** a column sort on top of that order; clicking the active header a third time restores it */
	const sort = new TableSort<(typeof roster)[number]>({
		player: { by: (r) => r.name },
		steamId: { by: (r) => r.e.steamId },
		source: { by: (r) => (r.e.member ? 'member' : 'list') },
		note: { by: (r) => r.e.reason },
		added: { by: (r) => (r.e.member ? null : r.e.addedAt), dir: 'desc' },
		// slots that never expire last, then the soonest to lapse first
		expires: { by: (r) => r.e.expiresAt ?? '\uffff' },
		servers: { by: (r) => r.e.servers.filter((s) => s.state === 'applied').length, dir: 'desc' }
	});
	let rows = $derived(
		sort.sorted(
			roster.filter((r) => matches(search, r.e.steamId, r.name, r.e.reason, r.e.addedByName))
		)
	);

	$effect(() => {
		const ids = servers.map((s) => s.id);
		return watchLive(ids, (v) => {
			playersOn[v.serverId] = v.ok
				? v.players.map((p) => ({ steamId: p.steamId, name: p.name }))
				: [];
		});
	});
	// Personas are looked up for the list's ids alone, so a lookup never re-runs on its own result.
	let listIds = $derived(
		entries
			.map((e) => e.steamId)
			.sort()
			.join(',')
	);
	$effect(() => {
		const ids = listIds.split(',').filter(Boolean);
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

	async function addReserved() {
		const steamId = newId.trim();
		if (!isSteamId(steamId)) {
			toast('请输入 17 位 SteamID64。', 'err');
			return;
		}
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>('POST', path, {
				steamId,
				reason: newReason.trim(),
				expiresAt: expiryIso(newExpiry, newCustom)
			});
			toast(describeSync(res.sync, `已为 ${steamId} 分配预留席位。`), 'ok', 8000);
			newId = '';
			newReason = '';
			newExpiry = '0';
			newCustom = '';
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function setMembersReserved(on: boolean) {
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'PATCH',
				`/api/orgs/${encodeURIComponent(org.id)}`,
				{ membersReserved: on }
			);
			toast(
				describeSync(res.sync, on ? '成员现在可获得预留席位。' : '成员预留席位已撤销。'),
				'ok',
				8000
			);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function syncNow() {
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'POST',
				`/api/orgs/${encodeURIComponent(org.id)}/lists/sync`
			);
			toast(describeSync(res.sync, '同步已执行。'), 'ok', 8000);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function remove(e: ListEntryView, name: string | null) {
		const label = name ? `${name} (${e.steamId})` : e.steamId;
		if (
			!(await confirmDialog(`确定从“${org.name}”组织撤回 ${label} 的预留席位？`, {
				okLabel: '撤回',
				danger: true
			}))
		)
			return;
		busy = true;
		try {
			const res = await api<{ sync: ListSyncSummary }>(
				'DELETE',
				`${path}/${encodeURIComponent(e.steamId)}`
			);
			toast(describeSync(res.sync, `已撤回 ${e.steamId} 的预留席位。`), 'ok', 8000);
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<div class="mb-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
	<div class="panel">
		<span class="label-sm">在名单中</span>
		<div class="flex items-baseline gap-2">
			<span class="font-display text-[34px] leading-none font-semibold tabular"
				>{entries.length}</span
			>
			{#if playing}
				<span class="ml-auto inline-flex items-center gap-1.5 text-[12.5px] text-ok"
					><span class="size-1.5 rounded-full bg-ok"></span>{playing} 当前在线</span
				>
			{/if}
		</div>
		<div class="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
			<span><b>{picked}</b> 手动预留</span>
			{#if lists.membersReserved || members}<span><b>{members}</b> 名成员</span>{/if}
		</div>
		<p class="note">
			名单中的玩家可跳过 {org.name}所有服务器的加入队列。名单没有长度上限；各服务器的
			MaxReservedSlots 只决定保留多少个玩家位。
		</p>
	</div>

	<div class="panel">
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<span class="label-sm mb-0!">在服务器上</span>
			<button class="ml-auto btn btn-sm" disabled={busy || !lists.servers.length} onclick={syncNow}
				>立即同步</button
			>
		</div>
		{#each standing as s (s.id)}
			<div class="kv items-start py-1.5 text-[13px]">
				<div class="min-w-0">
					{#if s.href}
						<a href={s.href} class="font-medium hover:text-accent hover:underline">{s.name}</a>
					{:else}
						<span class="font-medium">{s.name}</span>
					{/if}
					<div class="text-[12px] text-mist-400">
						{#if s.syncedAt}已同步 {fmtTime(s.syncedAt)}{:else}从未同步{/if}
						{#if s.lastError}<span class="text-danger"> · {s.lastError}</span>{/if}
					</div>
				</div>
				<span class="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap">
					<span><b>{s.applied}</b> <span class="text-mist-400">of {entries.length}</span></span>
					{#if s.pending}<Badge tone="warn">{s.pending} 等待中</Badge>{/if}
				</span>
			</div>
		{:else}
			<p class="text-[13px] text-mist-400">
				{org.name} 尚无服务器，因此无法同步名单。条目会保留，并在添加服务器后应用。
			</p>
		{/each}
	</div>

	<div class="flex flex-col panel">
		<span class="label-sm">在所有服务器预留位置</span>
		<form
			class="space-y-2"
			onsubmit={(e) => {
				e.preventDefault();
				void addReserved();
			}}
		>
			<div class="join join-wrap w-full">
				<input
					class="input font-mono"
					type="text"
					inputmode="numeric"
					placeholder="SteamID64…"
					maxlength="17"
					required
					bind:value={newId}
				/>
				<button type="submit" class="btn btn-primary" disabled={busy || !newId.trim()}>预留</button>
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
				bind:value={newReason}
			/>
			<div class="flex flex-wrap gap-2">
				<label class="block sm:w-40"
					><span class="field-label">到期时间</span><select class="input" bind:value={newExpiry}>
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
			已在以下组织的所有服务器分配： {org.name}当前服务器及以后新增的服务器都会分配。设有到期时间的预留位会由面板届时撤回。若只想在一台服务器分配，请使用该服务器的“预留位”标签页。
		</p>
		{#if owner}
			<label class="mt-3 flex items-start gap-2 border-t border-white/8 pt-3 text-[13px]">
				<input
					type="checkbox"
					class="mt-0.5"
					checked={lists.membersReserved}
					disabled={busy}
					onchange={(e) => setMembersReserved((e.currentTarget as HTMLInputElement).checked)}
				/>
				<span
					><b>成员将获得预留位。</b>
					<span class="block text-mist-400"
						>以下组织的每位成员： {org.name} 在账号页面关联 SteamID 的成员可在所有服务器获得预留位；受到封禁的成员会跳过。</span
					></span
				>
			</label>
		{/if}
	</div>
</div>

{#if owner}<ImportCandidates kind="reserve" {org} {owner} />{/if}

<div class="panel">
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0!">预留位玩家</span>
		<div class="flex w-full items-center gap-2 sm:ml-auto sm:w-auto sm:min-w-[320px]">
			<input
				class="input"
				type="search"
				placeholder="按名称、SteamID、备注或管理员筛选…"
				bind:value={search}
			/>
			<span class="shrink-0 text-[12.5px] whitespace-nowrap text-mist-600"
				>{entries.length} 个席位</span
			>
		</div>
	</div>
	{#if entries.length}
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<SortHeader {sort} key="player">玩家</SortHeader>
						<SortHeader {sort} key="steamId">SteamID64</SortHeader>
						<SortHeader {sort} key="source">来源</SortHeader>
						<SortHeader {sort} key="note">备注</SortHeader>
						<SortHeader {sort} key="added">添加时间</SortHeader>
						<SortHeader {sort} key="expires">到期时间</SortHeader>
						<SortHeader {sort} key="servers">服务器</SortHeader>
						<th></th>
					</tr>
				</thead>
				<tbody>
					{#each rows as r (r.e.id)}
						{@const e = r.e}
						<tr class={e.expired ? 'text-mist-400' : ''}>
							<td>
								<span class="inline-flex min-w-0 items-center gap-2.5">
									<span
										class="size-2 shrink-0 rounded-full {r.live
											? 'bg-ok ring-[3px] ring-ok/25'
											: 'bg-ink-700'}"
										title={r.live
											? `Playing now on ${r.live.servers.join(', ')}`
											: '当前不在服务器上'}
									></span>
									{#if r.avatar}<img
											src={r.avatar}
											alt=""
											class="size-5 shrink-0 rounded-sm"
											loading="lazy"
											referrerpolicy="no-referrer"
										/>{/if}
									{#if dossierBase}
										<a
											href="{dossierBase}/{e.steamId}"
											class="truncate font-medium hover:text-accent hover:underline {r.name
												? ''
												: 'text-mist-400 italic'}"
											title="打开玩家档案">{r.name ?? '尚未观测到'}</a
										>
									{:else}
										<span class="truncate font-medium {r.name ? '' : 'text-mist-400 italic'}"
											>{r.name ?? '尚未观测到'}</span
										>
									{/if}
								</span>
								{#if r.live}<div class="mt-0.5 pl-[18px] caps text-[10px] text-ok">
										正在游玩： {r.live.servers.join(', ')}
									</div>{/if}
							</td>
							<td class="font-mono text-[12.5px] text-mist-400">{e.steamId}</td>
							<td>
								{#if e.member}
									<Badge tone="accent">成员</Badge>
								{:else}
									<Badge tone="ok">列表</Badge>
								{/if}
							</td>
							<td class="max-w-[280px]">
								{#if e.reason}{e.reason}{:else}<span class="text-mist-600">—</span>{/if}
							</td>
							<td class="text-[12.5px] whitespace-nowrap">
								{#if e.member}
									<span class="text-mist-400">按成员身份</span>
								{:else}
									<div>{e.addedByName || '—'}</div>
									<div class="text-mist-400">{fmtTime(e.addedAt)}</div>
								{/if}
							</td>
							<td class="text-[12.5px] whitespace-nowrap">
								{#if !e.expiresAt}
									<span class="text-mist-600">从不</span>
								{:else if e.expired}
									<Badge tone="warn">已过期，正在撤回</Badge>
								{:else}
									{fmtTime(e.expiresAt)}
								{/if}
							</td>
							<td>
								<span class="inline-flex flex-wrap gap-1">
									{#each e.servers as s (s.serverId)}
										<span
											class="whitespace-nowrap"
											title="{s.serverName}: {STATE_TEXT[s.state]}{s.error ? ` — ${s.error}` : ''}"
										>
											<Badge tone={STATE_TONE[s.state]}>{s.serverName}</Badge>
										</span>
									{/each}
								</span>
							</td>
							<td class="text-right whitespace-nowrap">
								{#if !e.member}
									<button
										type="button"
										class="btn btn-sm btn-ghost"
										title="在所有服务器撤回此预留位"
										disabled={busy}
										onclick={() => remove(e, r.name)}>撤回</button
									>
								{/if}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="8" class="py-6 text-center text-mist-600">没有符合筛选条件的玩家。</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="note">
			<Badge tone="ok">列表</Badge> 个预留位在这里手动分配，可随时撤回； <Badge tone="accent"
				>成员</Badge
			> 个预留位随成员身份生效，成员退出或关闭上方开关后自动撤回。服务器标记： <Badge tone="ok"
				>已应用</Badge
			> 由面板管理， <Badge tone="warn">待处理</Badge> 下次同步时， <Badge tone="err">失败</Badge> （悬停查看原因），
			<Badge>本服</Badge> 该服务器上已有此条目，但由面板外部添加，因此面板不会移除。
		</p>
	{:else}
		<div class="callout mb-0">
			<b>尚无人持有预留位。</b>
			<span class="block text-mist-400"
				>预留位让管理员、捐赠者和战队成员在服务器满员时跳过队列。在上方分配后，面板会将其应用到 {org.name}.</span
			>
		</div>
	{/if}
</div>
