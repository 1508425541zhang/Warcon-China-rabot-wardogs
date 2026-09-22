<script lang="ts">
	import { api, qs, rconPost, errorMessage } from '$lib/api';
	import { watchLive } from '$lib/live';
	import { fmtNum } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { STATE_TONE } from '$lib/lists';
	import FactionChip from '$lib/components/FactionChip.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import BanDialog from '$lib/components/BanDialog.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { LiveView, Player, PlayerMark, ServerListsState, Status } from '$lib/types';
	import PastPlayers from './PastPlayers.svelte';
	import type { PageProps, Snapshot } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let moderate = $derived(can(data.server.caps, 'players.moderate'));
	let chat = $derived(can(data.server.caps, 'chat.send'));
	let bans = $derived(can(data.server.caps, 'bans.manage'));
	let anyAction = $derived(moderate || chat || bans);
	let notes = $derived(can(data.server.caps, 'players.notes'));
	/** who is on now, or everyone who has played here */
	let view = $state<'online' | 'past'>('online');
	/** may the user write to the organisation's lists? Decides the ban dialog's default scope. */
	let listState = $state<ServerListsState | null>(null);
	let banning = $state<Player | null>(null);

	let all = $state<Player[]>([]);
	let status = $state<Status | null>(null);
	let search = $state('');
	/** keep the filter across a trip to a dossier and back */
	export const snapshot: Snapshot<string> = {
		capture: () => search,
		restore: (v) => (search = v)
	};

	type Kind = 'whisper' | 'kick' | 'move';
	/** the one-field dialog an inline action opened; the player is a snapshot taken when it opened */
	let dialog = $state<{ kind: Kind; player: Player } | null>(null);
	let text = $state('');
	let team = $state('');
	let busy = $state(false);
	/** watchlist, first-visit and risk marks by SteamID; refreshed when the roster changes */
	let marks = $state<Record<string, PlayerMark>>({});
	let marksKey = '';
	let marksAt = 0;
	let base = $derived(`/server/${encodeURIComponent(data.server.id)}/players`);

	/** the roster in the worker's order until a header is clicked */
	const sort = new TableSort<Player>({
		player: { by: (p) => p.name },
		flags: {
			by: (p) => {
				const m = marks[p.steamId];
				return m ? (m.watched ? 1000 : 0) + m.risk.score + (m.firstVisit ? 1 : 0) : 0;
			},
			dir: 'desc'
		},
		reserved: { by: (p) => !!listState?.reserved[p.steamId], dir: 'desc' },
		faction: { by: (p) => p.faction },
		kills: { by: (p) => p.kills, dir: 'desc' },
		deaths: { by: (p) => p.deaths, dir: 'desc' },
		cash: { by: (p) => p.cash, dir: 'desc' },
		ping: { by: (p) => p.ping }
	});
	let rows = $derived(sort.sorted(all.filter((p) => matches(search, p.name, p.steamId))));
	/** the dialog's player as the roster sees them now; null once they have left */
	let live = $derived.by(() => {
		const d = dialog;
		return d ? (all.find((p) => p.steamId === d.player.steamId) ?? null) : null;
	});
	/** factions the dialog's player could be moved to */
	let destinations = $derived(
		(status?.scores ?? []).map((f) => f.name).filter((n) => n !== (live ?? dialog?.player)?.faction)
	);

	async function act(action: string, params: object) {
		try {
			const result = await rconPost<{ message?: string }>(id, action, params);
			toast(result?.message || `${action} done.`, 'ok');
			await refreshPlayers();
			return true;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return false;
		}
	}
	function onLive(v: LiveView) {
		all = v.players;
		if (v.status) status = v.status;
		void refreshMarks(v.players);
	}
	/** After a command the worker looks again by itself; this only refreshes the panel's own marks. */
	async function refreshPlayers() {
		marksKey = '';
		await Promise.all([refreshMarks(all), refreshListState()]);
	}
	async function refreshMarks(players: Player[]) {
		const ids = players.map((p) => p.steamId).filter((s) => /^\d{17}$/.test(s));
		const key = ids.join(',');
		if (!ids.length) {
			marks = {};
			marksKey = '';
			return;
		}
		if (key === marksKey && Date.now() - marksAt < 30000) return;
		marksKey = key;
		marksAt = Date.now();
		try {
			const d = await api<{ marks: PlayerMark[] }>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/players/marks${qs({ ids: key, names: players.map((p) => p.name).join('\n') })}`
			);
			const next: Record<string, PlayerMark> = {};
			for (const m of d.marks) next[m.steamId] = m;
			marks = next;
		} catch (err) {
			console.warn('marks', err);
		}
	}
	async function refreshListState() {
		try {
			listState = await api<ServerListsState>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/lists/state`
			);
		} catch (err) {
			console.warn('list state', err);
		}
	}
	$effect(() => {
		void id;
		void refreshListState();
		return watchLive([id], onLive);
	});

	function open(kind: Kind, player: Player) {
		if (busy) return;
		text = '';
		dialog = { kind, player };
		team = destinations[0] ?? '';
	}
	async function kill(p: Player) {
		if (busy) return;
		if (
			!(await confirmDialog(`Kill ${p.name}?`, {
				title: `Kill ${p.name}`,
				okLabel: 'Kill',
				danger: true
			}))
		)
			return;
		busy = true;
		try {
			await act('kill', { steamId: p.steamId });
		} finally {
			busy = false;
		}
	}
	/** Submits the open dialog: whisper, kick with reason, or move to the chosen faction. */
	async function submit() {
		const d = dialog;
		if (!d || busy || !live) return;
		const p = d.player;
		let ok = false;
		busy = true;
		try {
			if (d.kind === 'whisper') {
				const message = text.trim();
				if (!message) return;
				ok = await act('whisper', { steamId: p.steamId, message });
			} else if (d.kind === 'kick') {
				ok = await act('kick', { steamId: p.steamId, reason: text.trim() });
			} else {
				if (!destinations.includes(team)) return;
				ok = await act('changeTeam', { steamId: p.steamId, faction: team });
			}
		} finally {
			busy = false;
		}
		// Only close the dialog the request came from, not one opened since.
		if (ok && dialog === d) dialog = null;
	}
	const TITLES: Record<Kind, string> = { whisper: 'Whisper to', kick: 'Kick', move: 'Move' };
	const SUBMIT: Record<Kind, string> = { whisper: 'Send', kick: 'Kick', move: 'Move' };
</script>

<div class="panel">
	<div class="join mb-3" role="tablist" aria-label="Which players">
		<button
			class="btn {view === 'online' ? 'btn-primary' : ''}"
			role="tab"
			aria-selected={view === 'online'}
			onclick={() => (view = 'online')}>Online now · {all.length}</button
		>
		<button
			class="btn {view === 'past' ? 'btn-primary' : ''}"
			role="tab"
			aria-selected={view === 'past'}
			onclick={() => (view = 'past')}>Past players</button
		>
	</div>
	{#if view === 'past'}
		<PastPlayers
			server={{
				id,
				name: data.server.name,
				orgId: data.server.orgId,
				orgName: data.server.orgName
			}}
			canBan={bans}
			canWatch={notes}
			canOrg={listState?.canEditOrgBans ?? false}
		/>
	{:else}
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<div class="join w-full sm:w-auto sm:min-w-[320px]">
				<input
					class="input"
					type="search"
					placeholder="Filter by name or SteamID…"
					aria-label="Filter players"
					bind:value={search}
				/>
				<button class="btn" onclick={refreshPlayers}>Refresh</button>
			</div>
			<span class="ml-auto text-[12.5px] text-mist-600">{rows.length} / {all.length} players</span>
		</div>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<SortHeader {sort} key="player">Player</SortHeader>
						<SortHeader {sort} key="flags">Flags</SortHeader>
						<SortHeader {sort} key="reserved">Reserved</SortHeader>
						<SortHeader {sort} key="faction">Faction</SortHeader>
						<SortHeader {sort} key="kills" num>K</SortHeader>
						<SortHeader {sort} key="deaths" num>D</SortHeader>
						<SortHeader {sort} key="cash" num>Cash</SortHeader>
						<SortHeader {sort} key="ping" num>Ping</SortHeader>
						{#if anyAction}<th class="text-right">Actions</th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each rows as p (p.steamId)}
						{@const m = marks[p.steamId]}
						{@const r = listState?.reserved[p.steamId]}
						<tr>
							<td
								><a
									href="{base}/{p.steamId}"
									class="font-medium text-mist-100 underline decoration-mist-600 underline-offset-[3px] hover:text-accent hover:decoration-accent"
									>{p.name}</a
								>
								<span class="font-mono text-[12px] text-mist-600">{p.steamId}</span></td
							>
							<td class="whitespace-nowrap">
								{#if m}
									{#if m.watched}<Badge tone="warn" class="mr-1">watch</Badge>{/if}
									{#if m.risk.level === 'high'}<Badge tone="err" class="mr-1"
											>risk {m.risk.score}</Badge
										>{:else if m.risk.level === 'medium'}<Badge tone="warn" class="mr-1"
											>risk {m.risk.score}</Badge
										>{/if}
									{#if m.firstVisit}<Badge tone="info">new</Badge>{/if}
								{/if}
							</td>
							<td class="whitespace-nowrap">
								{#if r}
									{#if r.member}
										<Badge tone="accent">member</Badge>
									{:else if r.managed}
										<Badge tone={STATE_TONE[r.state]}
											>org{r.state === 'applied' ? '' : ` · ${r.state}`}</Badge
										>
									{:else}
										<Badge>local</Badge>
									{/if}
								{/if}
							</td>
							<td><FactionChip faction={p.faction} scores={status?.scores} /></td>
							<td class="num">{p.kills}</td><td class="num">{p.deaths}</td>
							<td class="num">{fmtNum(p.cash)}</td><td class="num">{p.ping ?? '—'}</td>
							{#if anyAction}
								<td class="py-1.5 text-right whitespace-nowrap">
									<div class="inline-flex gap-2">
										{#if chat || moderate}
											<div class="join">
												{#if chat}
													<button
														class="btn btn-sm"
														disabled={busy}
														aria-label="Whisper to {p.name}"
														onclick={() => open('whisper', p)}>Whisper</button
													>
												{/if}
												{#if moderate && data.features.changeTeam}
													<button
														class="btn btn-sm"
														disabled={busy}
														aria-label="Move {p.name} to another faction"
														onclick={() => open('move', p)}>Move</button
													>
												{/if}
												{#if moderate}
													<button
														class="btn btn-sm"
														disabled={busy}
														aria-label="Kill {p.name}"
														onclick={() => kill(p)}>Kill</button
													>
												{/if}
											</div>
										{/if}
										{#if moderate || bans}
											<div class="join">
												{#if moderate}
													<button
														class="btn btn-sm btn-danger"
														disabled={busy}
														aria-label="Kick {p.name}"
														onclick={() => open('kick', p)}>Kick</button
													>
												{/if}
												{#if bans}
													<button
														class="btn btn-sm btn-danger"
														disabled={busy}
														aria-label="Ban {p.name}"
														onclick={() => (banning = p)}>Ban</button
													>
												{/if}
											</div>
										{/if}
									</div>
								</td>
							{/if}
						</tr>
					{:else}
						<tr
							><td colspan={anyAction ? 9 : 8} class="py-6 text-center text-mist-600"
								>{all.length ? 'No matches.' : 'No players connected.'}</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		{#if !anyAction}<p class="note">You have view-only access; player actions are disabled.</p>{/if}
		<p class="note">
			{#if listState?.canEditOrgBans || listState?.canEditOrgSlots}Everyone who has ever joined is
				under
				<a
					href="/orgs/{encodeURIComponent(data.server.orgId)}/players?server={encodeURIComponent(
						id
					)}"
					class="text-accent hover:underline">Players seen</a
				>, with the names they used.
			{/if}This server's ban list is under
			<a href="/server/{encodeURIComponent(id)}/bans" class="text-accent hover:underline">Bans</a>
			and its reserved slots under
			<a href="/server/{encodeURIComponent(id)}/slots" class="text-accent hover:underline"
				>Reserved slots</a
			>.
		</p>
	{/if}
</div>

{#if dialog}
	{@const { kind, player: p } = dialog}
	{@const who = live ?? p}
	<Modal title="{TITLES[kind]} {p.name}" onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-wrap items-center gap-2 font-mono text-[12px] text-mist-400">
				<FactionChip faction={who.faction} scores={status?.scores} />
				{p.steamId}
				{#if !live}<Badge tone="err">left the server</Badge>{/if}
			</div>
			{#if kind === 'whisper'}
				<label class="sr-only" for="dialog-text">Message</label>
				<input
					id="dialog-text"
					class="input"
					type="text"
					placeholder="Private message…"
					maxlength="200"
					bind:value={text}
				/>
			{:else if kind === 'kick'}
				<label class="sr-only" for="dialog-text">Reason</label>
				<input
					id="dialog-text"
					class="input"
					type="text"
					placeholder="Reason (optional)…"
					maxlength="200"
					bind:value={text}
				/>
			{:else}
				<label class="sr-only" for="dialog-team">Faction</label>
				<select id="dialog-team" class="input" bind:value={team} disabled={!destinations.length}>
					{#each destinations as f (f)}<option value={f}>{f}</option>{/each}
				</select>
			{/if}
			<div class="flex justify-end gap-2">
				<button type="button" class="btn btn-ghost" data-close onclick={() => (dialog = null)}
					>Cancel</button
				>
				<button
					type="submit"
					class="btn {kind === 'kick' ? 'btn-danger' : 'btn-primary'}"
					disabled={busy ||
						!live ||
						(kind === 'whisper' && !text.trim()) ||
						(kind === 'move' && !destinations.includes(team))}>{SUBMIT[kind]}</button
				>
			</div>
		</form>
	</Modal>
{/if}

{#if banning}
	{#key banning.steamId}
		<BanDialog
			orgId={data.server.orgId}
			orgName={data.server.orgName}
			steamId={banning.steamId}
			name={banning.name}
			server={{ id, name: data.server.name }}
			canOrg={listState?.canEditOrgBans ?? false}
			banMessage={listState?.banMessage}
			onclose={() => (banning = null)}
			ondone={refreshPlayers}
		/>
	{/key}
{/if}
