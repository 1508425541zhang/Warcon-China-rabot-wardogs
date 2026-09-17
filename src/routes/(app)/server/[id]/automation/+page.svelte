<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime, mapLabel } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import MapPicker from '$lib/components/MapPicker.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import { watchLive } from '$lib/live';
	import type {
		DryRunResult,
		MapSelection,
		OutboxView,
		TriggerKind,
		TriggerView
	} from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let admin = $derived(can(data.server.caps, 'automation.manage'));
	let path = $derived(`/api/servers/${encodeURIComponent(id)}/triggers`);

	/** The last actions the rules took and what became of them; refreshed as deliveries happen. */
	let deliveries = $state<OutboxView[]>([]);
	let deliverySearch = $state('');
	const deliverySort = new TableSort<OutboxView>({
		when: { by: (d) => d.createdAt, dir: 'desc' },
		rule: { by: (d) => d.triggerName },
		action: { by: (d) => d.action },
		target: { by: (d) => d.target },
		state: { by: (d) => d.state },
		result: { by: (d) => d.outcome }
	});
	let deliveryRows = $derived(
		deliverySort.sorted(
			deliveries.filter((d) =>
				matches(deliverySearch, d.triggerName, d.action, d.target, d.state, d.outcome)
			)
		)
	);
	let deliveriesTimer: ReturnType<typeof setTimeout> | undefined;
	async function refreshDeliveries() {
		try {
			deliveries = (
				await api<{ items: OutboxView[] }>('GET', `/api/servers/${encodeURIComponent(id)}/outbox`)
			).items;
		} catch {
			/* shown as empty */
		}
	}
	$effect(() => {
		void id;
		void refreshDeliveries();
		return watchLive(
			[id],
			() => {},
			() => {
				clearTimeout(deliveriesTimer);
				deliveriesTimer = setTimeout(() => void refreshDeliveries(), 300);
			}
		);
	});
	const stateTone = (s: OutboxView['state']) =>
		s === 'delivered'
			? 'ok'
			: s === 'pending' || s === 'sending'
				? 'info'
				: s === 'skipped'
					? 'warn'
					: 'err';

	const KINDS: { kind: TriggerKind; label: string; blurb: string }[] = [
		{
			kind: 'welcome',
			label: 'Welcome whisper',
			blurb: 'Send a private message to players as they join or once they pick a faction.'
		},
		{
			kind: 'faction_change',
			label: 'Faction change whisper',
			blurb: 'Send a private message to players who switch from one faction to another.'
		},
		{
			kind: 'broadcast',
			label: 'Scheduled broadcast',
			blurb: 'Rotate through messages every few minutes while people are on.'
		},
		{
			kind: 'empty_reset',
			label: 'Empty-server map reset',
			blurb: 'Send an empty server back to a chosen map after a while.'
		},
		{
			kind: 'risk_kick',
			label: 'Kick on connect risk',
			blurb: 'Kick joiners with VAC bans, brand-new accounts, or bans elsewhere in the org.'
		},
		{
			kind: 'restart_notice',
			label: 'Restart notice',
			blurb:
				'Warn players before the game’s twelve-hour restart, and tell them once it will happen at the end of the round.'
		},
		{
			kind: 'team_kill',
			label: 'Team kill limit',
			blurb:
				'Whisper a player over team kills, and kick them past a limit. Needs the kill feed (Configuration tab).'
		}
	];
	const label = (kind: TriggerKind) => KINDS.find((k) => k.kind === kind)?.label ?? kind;

	interface Form {
		id: string | null;
		kind: TriggerKind;
		name: string;
		enabled: boolean;
		message: string;
		onlyFirstVisit: boolean;
		afterFaction: boolean;
		messages: string;
		everyMinutes: number;
		minPlayers: number;
		maxPlayers: number | '';
		afterMinutes: number;
		cooldownMinutes: number;
		vacBans: boolean;
		gameBans: boolean;
		minAccountDays: number;
		privateProfiles: boolean;
		bannedElsewhere: boolean;
		watchlist: boolean;
		spareReserved: boolean;
		reason: string;
		leadMinutes: number;
		leadMessage: string;
		repeatMinutes: number;
		warnAt: number;
		warnMessage: string;
		kickAt: number;
		kickReason: string;
	}
	let form = $state<Form | null>(null);
	let picker = $state<MapPicker>();
	let pendingSel = $state<Partial<MapSelection> | null>(null);
	let busy = $state(false);
	let dry = $state<DryRunResult | null>(null);
	let dryBusy = $state(false);
	let dryFor = $state<string | null>(null);

	$effect(() => {
		if (picker && pendingSel) {
			const sel = pendingSel;
			pendingSel = null;
			void picker.setFrom(sel);
		}
	});

	function open(kind: TriggerKind, t?: TriggerView) {
		const c = (t?.config ?? {}) as Record<string, unknown>;
		const s = (k: string, d: string) => (typeof c[k] === 'string' ? (c[k] as string) : d);
		const n = (k: string, d: number) => (typeof c[k] === 'number' ? (c[k] as number) : d);
		const b = (k: string, d: boolean) => (typeof c[k] === 'boolean' ? (c[k] as boolean) : d);
		form = {
			id: t?.id ?? null,
			kind,
			name: t?.name ?? label(kind),
			enabled: t?.enabled ?? true,
			message: s(
				'message',
				kind === 'faction_change'
					? 'You are now fighting for {faction}, {name}.'
					: kind === 'restart_notice'
						? 'Scheduled restart: the server restarts when this round ends. Rejoin in a minute or two.'
						: 'Welcome to {server}, {name}! Read the rules with /rules.'
			),
			onlyFirstVisit: b('onlyFirstVisit', false),
			afterFaction: b('afterFaction', false),
			messages: Array.isArray(c.messages)
				? (c.messages as string[]).join('\n')
				: 'Join our Discord for events and support.\nNo team-killing. Admins are watching.',
			everyMinutes: n('everyMinutes', 15),
			minPlayers: n('minPlayers', 1),
			maxPlayers: typeof c.maxPlayers === 'number' ? c.maxPlayers : '',
			afterMinutes: n('afterMinutes', 20),
			cooldownMinutes: n('cooldownMinutes', 30),
			vacBans: b('vacBans', true),
			gameBans: b('gameBans', false),
			minAccountDays: n('minAccountDays', 0),
			privateProfiles: b('privateProfiles', false),
			bannedElsewhere: b('bannedElsewhere', true),
			watchlist: b('watchlist', false),
			spareReserved: b('spareReserved', true),
			reason: s('reason', 'Your account does not meet this server’s requirements.'),
			leadMinutes: n('leadMinutes', 30),
			leadMessage: s(
				'leadMessage',
				'Scheduled restart in about {minutes} minutes, at the end of the round then in progress.'
			),
			repeatMinutes: n('repeatMinutes', 0),
			warnAt: n('warnAt', 2),
			warnMessage: s(
				'warnMessage',
				'Careful, {name}: that was a team kill ({count} this session).'
			),
			kickAt: n('kickAt', 4),
			kickReason: s('kickReason', 'Team killing ({count} this session).')
		};
		dry = null;
		pendingSel =
			kind === 'empty_reset' && t
				? {
						map: s('map', ''),
						experiences: Array.isArray(c.experiences) ? (c.experiences as string[]) : [],
						lighting: s('lighting', ''),
						zoneAlternator: s('zoneAlternator', '')
					}
				: null;
	}

	function config(f: Form): Record<string, unknown> {
		switch (f.kind) {
			case 'welcome':
				return {
					message: f.message,
					onlyFirstVisit: f.onlyFirstVisit,
					afterFaction: f.afterFaction
				};
			case 'faction_change':
				return { message: f.message };
			case 'broadcast':
				return {
					messages: f.messages.split('\n'),
					everyMinutes: Number(f.everyMinutes),
					minPlayers: Number(f.minPlayers),
					maxPlayers: f.maxPlayers === '' ? null : Number(f.maxPlayers)
				};
			case 'empty_reset':
				return {
					...(picker?.selection() ?? {}),
					afterMinutes: Number(f.afterMinutes),
					cooldownMinutes: Number(f.cooldownMinutes)
				};
			case 'risk_kick':
				return {
					vacBans: f.vacBans,
					gameBans: f.gameBans,
					minAccountDays: Number(f.minAccountDays),
					privateProfiles: f.privateProfiles,
					bannedElsewhere: f.bannedElsewhere,
					watchlist: f.watchlist,
					spareReserved: f.spareReserved,
					reason: f.reason
				};
			case 'restart_notice':
				return {
					message: f.message,
					leadMinutes: Number(f.leadMinutes),
					leadMessage: f.leadMessage,
					repeatMinutes: Number(f.repeatMinutes),
					minPlayers: Number(f.minPlayers)
				};
			case 'team_kill':
				return {
					warnAt: Number(f.warnAt),
					warnMessage: f.warnMessage,
					kickAt: Number(f.kickAt),
					kickReason: f.kickReason
				};
		}
	}

	async function run(fn: () => Promise<unknown>, done: string) {
		busy = true;
		try {
			await fn();
			if (done) toast(done, 'ok');
			await invalidateAll();
			return true;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return false;
		} finally {
			busy = false;
		}
	}
	async function save() {
		const f = form;
		if (!f) return;
		const body = { name: f.name.trim(), enabled: f.enabled, config: config(f) };
		const ok = await run(
			() =>
				f.id ? api('PATCH', `${path}/${f.id}`, body) : api('POST', path, { kind: f.kind, ...body }),
			f.id ? 'Trigger saved.' : 'Trigger added.'
		);
		if (ok) form = null;
	}
	const toggle = (t: TriggerView) =>
		run(
			() => api('PATCH', `${path}/${t.id}`, { enabled: !t.enabled }),
			t.enabled ? `${t.name} is off.` : `${t.name} is on.`
		);
	async function remove(t: TriggerView) {
		if (
			!(await confirmDialog(`Delete the trigger "${t.name}"?`, { okLabel: 'Delete', danger: true }))
		)
			return;
		await run(() => api('DELETE', `${path}/${t.id}`), 'Trigger deleted.');
	}
	async function dryRun(kind: TriggerKind, cfg: Record<string, unknown>, key: string) {
		dryBusy = true;
		dryFor = key;
		try {
			dry = (await api<{ result: DryRunResult }>('POST', `${path}/dry-run`, { kind, config: cfg }))
				.result;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			dryBusy = false;
		}
	}

	function describe(t: TriggerView): string {
		const c = t.config as Record<string, unknown>;
		switch (t.kind) {
			case 'welcome':
				return `"${c.message}"${c.afterFaction ? ' · after faction pick' : ' · on join'}${c.onlyFirstVisit ? ' · first visit only' : ''}`;
			case 'faction_change':
				return `"${c.message}"`;
			case 'broadcast':
				return `${(c.messages as string[]).length} message${(c.messages as string[]).length === 1 ? '' : 's'} every ${c.everyMinutes} min · ${typeof c.maxPlayers === 'number' ? `${c.minPlayers} to ${c.maxPlayers}` : `at least ${c.minPlayers}`} on`;
			case 'empty_reset':
				return `to ${mapLabel(data.catalog, String(c.map))} after ${c.afterMinutes} min empty`;
			case 'risk_kick': {
				const rules = [
					c.vacBans && 'VAC ban',
					c.gameBans && 'game ban',
					c.minAccountDays &&
						`account under ${c.minAccountDays} days${c.privateProfiles ? ' or private' : ''}`,
					c.bannedElsewhere && 'banned elsewhere in the org',
					c.watchlist && 'watchlist'
				].filter(Boolean);
				return `${rules.join(', ')}${c.spareReserved ? ' · spares reserved slots' : ''}`;
			}
			case 'restart_notice':
				return `"${c.message}"${c.leadMinutes ? ` · heads-up ${c.leadMinutes} min before` : ''}${c.repeatMinutes ? ` · again every ${c.repeatMinutes} min` : ''} · at least ${c.minPlayers} on`;
			case 'team_kill':
				return [
					c.warnAt ? `whisper from ${c.warnAt} team kill${c.warnAt === 1 ? '' : 's'}` : '',
					c.kickAt ? `kick at ${c.kickAt}` : ''
				]
					.filter(Boolean)
					.join(' · ')
					.concat(' · per session');
		}
	}
</script>

<div class="mb-4 flex flex-wrap items-center gap-2">
	<p class="text-[13px] text-mist-400">
		Rules the worker evaluates on every observation{data.server.demo ? ' of the demo server' : ''}:
		a join is acted on within a couple of seconds. Every action is queued, delivered, and recorded
		below and in the audit trail as
		<span class="chip">trigger</span>. Dry-run a rule against the last 24 hours before it touches
		anyone.
	</p>
</div>

{#if admin}
	<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
		{#each KINDS as k (k.kind)}
			<button
				type="button"
				class="cursor-pointer panel text-left transition hover:border-accent/60"
				onclick={() => open(k.kind)}
			>
				<div class="caps text-accent">+ {k.label}</div>
				<div class="mt-1 text-[13px] text-mist-400">{k.blurb}</div>
			</button>
		{/each}
	</div>
{/if}

<div class="space-y-3">
	{#each data.triggers as t (t.id)}
		<div class="panel {t.enabled ? '' : 'opacity-70'}">
			<div class="flex flex-wrap items-start gap-3">
				<label class="mt-0.5 inline-flex items-center gap-2">
					<input
						type="checkbox"
						checked={t.enabled}
						disabled={!admin || busy}
						onchange={() => toggle(t)}
					/>
				</label>
				<div class="min-w-0 flex-1">
					<div class="flex flex-wrap items-center gap-2">
						<span class="font-semibold">{t.name}</span>
						<Badge tone={t.enabled ? 'ok' : ''}>{t.enabled ? 'on' : 'off'}</Badge>
						<Badge tone="info">{label(t.kind)}</Badge>
					</div>
					<div class="mt-1 text-[13px] text-mist-400">{describe(t)}</div>
					<div class="mt-1 text-[12px] text-mist-600">
						{#if t.lastFiredAt}Last fired {fmtTime(t.lastFiredAt)} · {t.lastResult}{:else if t.lastResult}{t.lastResult}{:else}Never
							fired.{/if}
						{#if t.fireCount}· {t.fireCount} action{t.fireCount === 1 ? '' : 's'} so far{/if}
					</div>
				</div>
				{#if admin}
					<span class="inline-flex flex-wrap gap-1.5">
						<button
							class="btn btn-sm"
							disabled={dryBusy}
							onclick={() => dryRun(t.kind, t.config, t.id)}>Dry run</button
						>
						<button class="btn btn-sm" onclick={() => open(t.kind, t)}>Edit</button>
						<button class="btn btn-sm btn-danger" disabled={busy} onclick={() => remove(t)}
							>Delete</button
						>
					</span>
				{/if}
			</div>
			{#if dry && dryFor === t.id && !form}
				<div class="mt-3 rounded-ctl border border-black bg-ink-950 p-3">
					{@render dryResult(dry)}
				</div>
			{/if}
		</div>
	{:else}
		<div class="panel text-center text-mist-600">
			No triggers yet.{#if admin}
				Pick one above to start.{/if}
		</div>
	{/each}
</div>

{#snippet dryResult(r: DryRunResult)}
	<div class="mb-2 flex flex-wrap items-center gap-2 text-[13px]">
		{#if r.kind === 'restart_notice'}
			<b>Next cycle:</b>
			<span
				><b class={r.fires ? 'text-warn' : 'text-ok'}>{r.fires}</b> broadcast{r.fires === 1
					? ''
					: 's'}</span
			>
		{:else}
			<b>Dry run, last 24 h:</b>
			<span
				>would have fired <b class={r.fires ? 'text-warn' : 'text-ok'}>{r.fires}</b>
				time{r.fires === 1 ? '' : 's'}</span
			>
		{/if}
		<button type="button" class="ml-auto btn btn-sm btn-ghost" onclick={() => (dry = null)}
			>✕</button
		>
	</div>
	{#if r.items.length}
		<ul class="max-h-56 space-y-0.5 overflow-y-auto font-mono text-[12px]">
			{#each r.items as it, i (i)}
				<li><span class="text-mist-600">{fmtTime(it.at)}</span> {it.text}</li>
			{/each}
			{#if r.fires > r.items.length}<li class="text-mist-600">
					… and {r.fires - r.items.length} more
				</li>{/if}
		</ul>
	{/if}
	{#each r.notes as n (n)}<p class="note">{n}</p>{/each}
{/snippet}

{#if form}
	{@const f = form}
	<Modal
		title="{f.id ? 'Edit' : 'New'} · {label(f.kind)}"
		wide={f.kind === 'empty_reset'}
		onclose={() => (form = null)}
	>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				save();
			}}
		>
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-[1fr_auto]">
				<label class="block"
					><span class="field-label">Name</span><input
						class="input"
						type="text"
						bind:value={f.name}
						maxlength="60"
						required
					/></label
				>
				<label class="flex items-end gap-2 pb-2.5 text-[13px]"
					><input type="checkbox" bind:checked={f.enabled} /> Enabled</label
				>
			</div>

			{#if f.kind === 'welcome'}
				<label class="block"
					><span class="field-label">Message</span><input
						class="input"
						type="text"
						bind:value={f.message}
						maxlength="200"
						required
					/></label
				>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={f.afterFaction} /> Wait until the player has picked a
					faction</label
				>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={f.onlyFirstVisit} /> Only on a player's first visit to
					this server</label
				>
				<p class="note">
					Placeholders: <span class="chip">{'{name}'}</span> <span class="chip">{'{faction}'}</span>
					<span class="chip">{'{server}'}</span> <span class="chip">{'{map}'}</span>
					<span class="chip">{'{players}'}</span> <span class="chip">{'{max}'}</span>. Sent as a
					whisper, so only that player sees it.
				</p>
			{:else if f.kind === 'faction_change'}
				<label class="block"
					><span class="field-label">Message</span><input
						class="input"
						type="text"
						bind:value={f.message}
						maxlength="200"
						required
					/></label
				>
				<p class="note">
					Fires when a player moves from one faction to another, not on their first pick after
					joining. Placeholders: <span class="chip">{'{name}'}</span>
					<span class="chip">{'{faction}'}</span> <span class="chip">{'{previous}'}</span>
					<span class="chip">{'{server}'}</span> <span class="chip">{'{map}'}</span>
					<span class="chip">{'{players}'}</span> <span class="chip">{'{max}'}</span>.
				</p>
			{:else if f.kind === 'broadcast'}
				<label class="block"
					><span class="field-label">Messages (one per line, sent in turn)</span><textarea
						class="min-h-[100px] input"
						bind:value={f.messages}
						required></textarea></label
				>
				<div class="grid grid-cols-2 gap-3">
					<label class="block"
						><span class="field-label">Every (minutes)</span><input
							class="input"
							type="number"
							min="1"
							max="1440"
							bind:value={f.everyMinutes}
							required
						/></label
					>
					<label class="block"
						><span class="field-label">Only with at least (players)</span><input
							class="input"
							type="number"
							min="0"
							max="1000"
							bind:value={f.minPlayers}
						/></label
					>
					<label class="block"
						><span class="field-label">And at most (players, blank for no ceiling)</span><input
							class="input"
							type="number"
							min="0"
							max="1000"
							bind:value={f.maxPlayers}
						/></label
					>
				</div>
				<p class="note">
					Same placeholders as the welcome whisper, minus <span class="chip">{'{name}'}</span>.
					Broadcasts are limited to 200 characters.
				</p>
			{:else if f.kind === 'empty_reset'}
				<MapPicker bind:this={picker} serverId={id} catalog={data.catalog} />
				<div class="grid grid-cols-2 gap-3">
					<label class="block"
						><span class="field-label">After empty for (minutes)</span><input
							class="input"
							type="number"
							min="1"
							max="1440"
							bind:value={f.afterMinutes}
							required
						/></label
					>
					<label class="block"
						><span class="field-label">Cooldown between resets (minutes)</span><input
							class="input"
							type="number"
							min="1"
							max="1440"
							bind:value={f.cooldownMinutes}
						/></label
					>
				</div>
				<p class="note">
					Fires when nobody has been on for that long and the server is on a different map or mode.
					With a rotation the target is set as next and the match ended; without one the map is
					requested directly.
				</p>
			{:else if f.kind === 'restart_notice'}
				<label class="block"
					><span class="field-label">Message once the window is open</span><input
						class="input"
						type="text"
						bind:value={f.message}
						maxlength="200"
						required
					/></label
				>
				<label class="block"
					><span class="field-label">Heads-up message</span><input
						class="input"
						type="text"
						bind:value={f.leadMessage}
						maxlength="200"
						disabled={!Number(f.leadMinutes)}
					/></label
				>
				<div class="grid grid-cols-3 gap-3">
					<label class="block"
						><span class="field-label">Heads-up, min before</span><input
							class="input"
							type="number"
							min="0"
							max="719"
							bind:value={f.leadMinutes}
						/></label
					>
					<label class="block"
						><span class="field-label">Repeat every, min</span><input
							class="input"
							type="number"
							min="0"
							max="1440"
							bind:value={f.repeatMinutes}
						/></label
					>
					<label class="block"
						><span class="field-label">At least, players</span><input
							class="input"
							type="number"
							min="0"
							max="1000"
							bind:value={f.minPlayers}
						/></label
					>
				</div>
				<p class="note">
					The game restarts twelve hours after it started, once the round then in progress ends. The
					heads-up goes that many minutes before the window opens (0 turns it off); the main message
					goes once it has, and again on the repeat cadence while the round runs on (0 sends it
					once). Placeholders: <span class="chip">{'{minutes}'}</span>
					<span class="chip">{'{uptime}'}</span> <span class="chip">{'{server}'}</span>
					<span class="chip">{'{map}'}</span> <span class="chip">{'{players}'}</span>
					<span class="chip">{'{max}'}</span>.
				</p>
			{:else if f.kind === 'team_kill'}
				<div class="grid grid-cols-2 gap-3">
					<label class="block"
						><span class="field-label">Whisper from, team kills</span><input
							class="input"
							type="number"
							min="0"
							max="100"
							bind:value={f.warnAt}
						/></label
					>
					<label class="block"
						><span class="field-label">Kick at, team kills</span><input
							class="input"
							type="number"
							min="0"
							max="100"
							bind:value={f.kickAt}
						/></label
					>
				</div>
				<label class="block"
					><span class="field-label">Whisper</span><input
						class="input"
						type="text"
						bind:value={f.warnMessage}
						maxlength="200"
						disabled={!Number(f.warnAt)}
					/></label
				>
				<label class="block"
					><span class="field-label">Kick reason</span><input
						class="input"
						type="text"
						bind:value={f.kickReason}
						maxlength="200"
						disabled={!Number(f.kickAt)}
					/></label
				>
				<p class="note">
					Team kills come from the game's kill feed (set up on the Configuration tab) and are
					counted per player within their current session. The whisper goes on every team kill from
					the first threshold on; 0 turns either action off. Placeholders: <span class="chip"
						>{'{name}'}</span
					>
					<span class="chip">{'{victim}'}</span> <span class="chip">{'{count}'}</span>
					<span class="chip">{'{server}'}</span> <span class="chip">{'{map}'}</span>.
				</p>
			{/if}

			{#if dry && dryFor === 'form'}
				<div class="rounded-ctl border border-black bg-ink-950 p-3">{@render dryResult(dry)}</div>
			{/if}

			<div class="flex flex-wrap justify-end gap-2 pt-2">
				<button
					type="button"
					class="mr-auto btn"
					disabled={dryBusy}
					onclick={() => dryRun(f.kind, config(f), 'form')}
					>{dryBusy
						? 'Working…'
						: f.kind === 'restart_notice'
							? 'Preview next cycle'
							: 'Dry run (last 24 h)'}</button
				>
				<button type="button" class="btn" data-close onclick={() => (form = null)}>Cancel</button>
				<button type="submit" class="btn btn-primary" disabled={busy}
					>{f.id ? 'Save' : 'Add trigger'}</button
				>
			</div>
		</form>
	</Modal>
{/if}

<div class="mt-4 panel">
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<span class="label-sm mb-0">Recent actions</span>
		<span class="text-[12.5px] text-mist-600"
			>what the rules did, newest first · <b>unknown</b> means sent with no answer, never retried on its
			own</span
		>
		<input
			class="input w-full sm:ml-auto sm:w-64"
			type="search"
			placeholder="Filter by rule, action, target, state…"
			aria-label="Filter recent actions"
			bind:value={deliverySearch}
		/>
	</div>
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<SortHeader sort={deliverySort} key="when">When</SortHeader>
					<SortHeader sort={deliverySort} key="rule">Rule</SortHeader>
					<SortHeader sort={deliverySort} key="action">Action</SortHeader>
					<SortHeader sort={deliverySort} key="target">Target</SortHeader>
					<SortHeader sort={deliverySort} key="state">State</SortHeader>
					<SortHeader sort={deliverySort} key="result">Result</SortHeader>
				</tr>
			</thead>
			<tbody>
				{#each deliveryRows as d (d.id)}
					<tr>
						<td class="whitespace-nowrap">{fmtTime(d.createdAt)}</td>
						<td>{d.triggerName}</td>
						<td class="font-mono text-[12px]">{d.action}</td>
						<td class="font-mono text-[12px]">{d.target}</td>
						<td><Badge tone={stateTone(d.state)}>{d.state}</Badge></td>
						<td class="text-mist-400">{d.outcome}</td>
					</tr>
				{:else}
					<tr
						><td colspan="6" class="py-6 text-center text-mist-600"
							>{deliveries.length ? 'Nothing matches.' : 'No actions yet.'}</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
</div>
