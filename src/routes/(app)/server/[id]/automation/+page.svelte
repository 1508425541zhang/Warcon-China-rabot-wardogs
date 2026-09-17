<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtAgo, fmtSpan, fmtTime, mapLabel } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import MapPicker from '$lib/components/MapPicker.svelte';
	import RowMenu from '$lib/components/RowMenu.svelte';
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

	// The kinds, grouped by what they act on for the Add menu. `needs` is what a kind must have
	// before it can run here, shown in the menu and at the top of its editor; '' when it can.
	type Group = 'Messages' | 'Players' | 'Server';
	const KINDS: { kind: TriggerKind; group: Group; label: string; blurb: string }[] = [
		{
			kind: 'welcome',
			group: 'Messages',
			label: 'Welcome whisper',
			blurb: 'Whisper players as they join, or once they pick a faction.'
		},
		{
			kind: 'faction_change',
			group: 'Messages',
			label: 'Faction change whisper',
			blurb: 'Whisper players who switch sides.'
		},
		{
			kind: 'broadcast',
			group: 'Messages',
			label: 'Scheduled broadcast',
			blurb: 'Rotate through messages every few minutes while people are on.'
		},
		{
			kind: 'restart_notice',
			group: 'Messages',
			label: 'Restart notice',
			blurb: 'Warn players before the twelve-hour restart and tell them when it lands.'
		},
		{
			kind: 'risk_kick',
			group: 'Players',
			label: 'Kick on connect risk',
			blurb: 'Kick joiners the panel already distrusts, before they get a slot.'
		},
		{
			kind: 'team_kill',
			group: 'Players',
			label: 'Team kill limit',
			blurb: 'Whisper a player about team kills and kick them past a limit.'
		},
		{
			kind: 'seed_reward',
			group: 'Players',
			label: 'Seeding reward',
			blurb: 'Give players who stay while the server is quiet a reserved slot.'
		},
		{
			kind: 'empty_reset',
			group: 'Server',
			label: 'Empty-server map reset',
			blurb: 'Put an empty server back on a chosen map after a while.'
		}
	];
	const GROUPS: Group[] = ['Messages', 'Players', 'Server'];
	const label = (kind: TriggerKind) => KINDS.find((k) => k.kind === kind)?.label ?? kind;
	const blurb = (kind: TriggerKind) => KINDS.find((k) => k.kind === kind)?.blurb ?? '';
	/** Why a kind cannot run on this server yet, or '' when it can. */
	let needs = $derived((kind: TriggerKind): string => {
		switch (kind) {
			case 'team_kill':
				return data.feed
					? ''
					: 'Needs the kill feed, which is off on this server. Turn it on under Configuration.';
			case 'risk_kick':
				return data.steam
					? ''
					: 'Steam lookup is off on this panel, so only the ban-list and watchlist rows can run.';
			case 'seed_reward':
				return can(data.server.caps, 'lists.edit')
					? ''
					: 'Saving needs the Org lists capability as well as Automation.';
			default:
				return '';
		}
	});
	/** A kind that lacks what it needs stays in the menu, greyed, with the reason in a few words. */
	const short = (kind: TriggerKind): string =>
		kind === 'team_kill' ? 'needs the kill feed' : kind === 'risk_kick' ? 'needs a Steam key' : '';
	let addOpen = $state(false);

	// The status lines count up on their own: a minute clock, only while the page is open.
	let now = $state(Date.now());
	$effect(() => {
		const t = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(t);
	});
	interface Health {
		/** the newest delivery for the rule failed, with nothing delivered since */
		failing: boolean;
		latest: string;
		outcome: string;
		/** deliveries since midnight, or in the loaded window when that is shorter */
		count: number;
	}
	/**
	 * What the loaded deliveries say about each rule. A trigger's own `lastResult` records the
	 * intent ("Kicking 2 players"), not what became of it, so health comes from the outbox rows the
	 * page already has: the server's last 40. A busy rule can push a quiet rule's rows out of that
	 * window, in which case the quiet rule shows its plain "Fired" line, which is honest.
	 */
	let health = $derived.by(() => {
		const byRule = new Map<string, Health>();
		const rows = [...deliveries].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
		const since = Math.max(new Date(now).setHours(0, 0, 0, 0), windowStart);
		for (const d of rows) {
			if (!d.triggerId) continue;
			const h = byRule.get(d.triggerId);
			const today = Date.parse(d.createdAt) >= since ? 1 : 0;
			if (h) h.count += today;
			else
				byRule.set(d.triggerId, {
					failing: d.state === 'failed',
					latest: d.createdAt,
					outcome: d.outcome,
					count: today
				});
		}
		return byRule;
	});
	/** when the oldest loaded delivery happened; the counts cannot see past it */
	let windowStart = $derived(
		deliveries.reduce((min, d) => Math.min(min, Date.parse(d.createdAt)), Infinity)
	);
	/** the outbox route's page size: fewer rows than that means the window holds everything */
	const OUTBOX_PAGE = 40;
	let coversToday = $derived(
		deliveries.length < OUTBOX_PAGE || windowStart <= new Date(now).setHours(0, 0, 0, 0)
	);
	/** "31 today", or "31 in the last 3 h" when the loaded window is shorter than the day */
	const countLine = (h: Health) =>
		coversToday ? `${h.count} today` : `${h.count} in the last ${fmtSpan(now - windowStart)}`;
	let failingCount = $derived(data.triggers.filter((t) => health.get(t.id)?.failing).length);
	let lastAction = $derived(
		deliveries.reduce<string | null>(
			(max, d) => (!max || d.createdAt > max ? d.createdAt : max),
			null
		)
	);
	let onlyFailing = $state(false);
	let rows = $derived(
		onlyFailing ? data.triggers.filter((t) => health.get(t.id)?.failing) : data.triggers
	);

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
		kickAtLevel: '' | 'medium' | 'high';
		spareReserved: boolean;
		reason: string;
		leadMinutes: number;
		leadMessage: string;
		repeatMinutes: number;
		warnAt: number;
		warnMessage: string;
		kickAt: number;
		kickReason: string;
		lowAt: number;
		untilFull: boolean;
		fullAt: number | '';
		minutes: number;
		windowDays: number;
		slotDays: number;
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

	/** The editor for a new rule of a kind, an existing rule, or a copy of one (`copy`). */
	function open(kind: TriggerKind, t?: TriggerView, copy = false) {
		const c = (t?.config ?? {}) as Record<string, unknown>;
		const s = (k: string, d: string) => (typeof c[k] === 'string' ? (c[k] as string) : d);
		const n = (k: string, d: number) => (typeof c[k] === 'number' ? (c[k] as number) : d);
		const b = (k: string, d: boolean) => (typeof c[k] === 'boolean' ? (c[k] as boolean) : d);
		form = {
			id: copy ? null : (t?.id ?? null),
			kind,
			name: t ? (copy ? `${t.name} (copy)` : t.name) : label(kind),
			enabled: t?.enabled ?? true,
			message: s(
				'message',
				kind === 'faction_change'
					? 'You are now fighting for {faction}, {name}.'
					: kind === 'restart_notice'
						? 'Scheduled restart: the server restarts when this round ends. Rejoin in a minute or two.'
						: kind === 'seed_reward'
							? 'Thanks for seeding {server}, {name}: you have a reserved slot until {until}.'
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
			kickAtLevel: c.kickAtLevel === 'high' || c.kickAtLevel === 'medium' ? c.kickAtLevel : '',
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
			kickReason: s('kickReason', 'Team killing ({count} this session).'),
			lowAt: n('lowAt', 20),
			untilFull: b('untilFull', true),
			fullAt: typeof c.fullAt === 'number' ? c.fullAt : '',
			minutes: n('minutes', 60),
			windowDays: n('windowDays', 7),
			slotDays: n('slotDays', 7)
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
					kickAtLevel: f.kickAtLevel || null,
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
			case 'seed_reward':
				return {
					lowAt: Number(f.lowAt),
					untilFull: f.untilFull,
					fullAt: f.fullAt === '' ? null : Number(f.fullAt),
					minutes: Number(f.minutes),
					windowDays: Number(f.windowDays),
					slotDays: Number(f.slotDays),
					message: f.message
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
					c.watchlist && 'watchlist',
					c.kickAtLevel && `${c.kickAtLevel}${c.kickAtLevel === 'medium' ? ' or high' : ''} risk`
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
			case 'seed_reward':
				return `${c.minutes} min with ${c.lowAt} or fewer on${c.untilFull === false ? '' : `, staying until ${typeof c.fullAt === 'number' ? `${c.fullAt}+ on` : 'it fills'}`}, within ${c.windowDays} day${c.windowDays === 1 ? '' : 's'} · slot for ${c.slotDays} day${c.slotDays === 1 ? '' : 's'}${c.message ? ' · whispers' : ''}`;
		}
	}
</script>

<svelte:window
	onclick={() => (addOpen = false)}
	onkeydown={(e) => e.key === 'Escape' && (addOpen = false)}
/>

<p class="mb-4 text-[13px] text-mist-400">
	Rules the worker evaluates on every observation{data.server.demo ? ' of the demo server' : ''}: a
	join is acted on within a couple of seconds. Every action is queued, delivered, and recorded below
	and in the audit trail as <span class="chip">trigger</span>. Dry-run a rule against the last 24
	hours before it touches anyone.
</p>

<div class="mb-3 flex flex-wrap items-start gap-3">
	<div class="min-w-0 grow">
		<span class="label-sm mb-0">Rules</span>
		<div class="mt-0.5 text-[12.5px] text-mist-400">
			{#if data.triggers.length}
				{data.triggers.length} rule{data.triggers.length === 1 ? '' : 's'} · {data.triggers.filter(
					(t) => t.enabled
				).length} on
				{#if lastAction}· last action <span title={fmtTime(lastAction)}
						>{fmtAgo(lastAction, now)}</span
					>{/if}
				{#if failingCount}
					· <button
						type="button"
						class="cursor-pointer text-danger underline decoration-danger/50 underline-offset-2 hover:decoration-danger"
						aria-pressed={onlyFailing}
						onclick={() => (onlyFailing = !onlyFailing)}
						>{failingCount} failing{onlyFailing ? ' · show all' : ''}</button
					>
				{/if}
			{:else}
				No rules on this server yet
			{/if}
		</div>
	</div>
	{#if admin}
		<div class="relative">
			<button
				type="button"
				class="btn gap-1.5 pr-2.5"
				aria-haspopup="menu"
				aria-expanded={addOpen}
				onclick={(e) => {
					e.stopPropagation();
					addOpen = !addOpen;
				}}
			>
				Add rule <span class="text-[10px] text-mist-600">▼</span>
			</button>
			{#if addOpen}
				<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
				<div
					class="absolute top-[calc(100%+6px)] right-0 z-40 min-w-[270px] rise rounded-card border border-black bg-ink-900 p-1 shadow-pop"
					role="menu"
					tabindex="-1"
					onclick={(e) => e.stopPropagation()}
				>
					{#each GROUPS as g (g)}
						<div class="px-3 pt-2 pb-1 caps text-mist-600">{g}</div>
						{#each KINDS.filter((k) => k.group === g) as k (k.kind)}
							<button
								type="button"
								class="menu-item {needs(k.kind) ? 'text-mist-600!' : ''}"
								role="menuitem"
								title={k.blurb}
								onclick={() => {
									addOpen = false;
									open(k.kind);
								}}
							>
								<span>{k.label}</span>
								{#if needs(k.kind) && short(k.kind)}
									<span class="ml-auto text-[11px] text-mist-600">{short(k.kind)}</span>
								{/if}
							</button>
						{/each}
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>

<div class="space-y-2">
	{#each rows as t (t.id)}
		{@const h = health.get(t.id)}
		<div class="panel py-3.5 {t.enabled ? '' : 'opacity-60'}">
			<div class="flex items-start gap-3">
				<button
					type="button"
					role="switch"
					aria-checked={t.enabled}
					aria-label="{t.name}: {t.enabled ? 'on' : 'off'}"
					class="mt-1 h-[18px] w-8 shrink-0 cursor-pointer rounded-full border border-black transition disabled:cursor-not-allowed {t.enabled
						? 'bg-accent'
						: 'bg-ink-700'}"
					disabled={!admin || busy}
					onclick={() => toggle(t)}
				>
					<span
						class="block h-3 w-3 rounded-full bg-ink-950 transition-transform {t.enabled
							? 'translate-x-[15px]'
							: 'translate-x-[2px]'}"
					></span>
				</button>
				<div class="min-w-0 flex-1">
					<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
						{#if admin}
							<button
								type="button"
								class="cursor-pointer text-left font-semibold hover:text-white"
								onclick={() => open(t.kind, t)}>{t.name}</button
							>
						{:else}
							<span class="font-semibold">{t.name}</span>
						{/if}
						{#if h?.failing}<Badge tone="err">▲ failing</Badge>{/if}
						<span class="chip">{label(t.kind)}</span>
					</div>
					<div class="mt-0.5 line-clamp-2 text-[13px] text-mist-400">{describe(t)}</div>
					<!-- One of four shapes, most urgent first: failing, off, fired, never fired. -->
					<div class="mt-0.5 text-[12px] {h?.failing ? 'text-mist-100' : 'text-mist-600'}">
						{#if h?.failing}
							Latest actions failed · <span class="font-mono text-[11.5px] text-mist-400"
								>{h.outcome}</span
							>
							· <span title={fmtTime(h.latest)}>{fmtAgo(h.latest, now)}</span>
						{:else if !t.enabled}
							Off · {#if t.lastFiredAt}last fired <span title={fmtTime(t.lastFiredAt)}
									>{fmtAgo(t.lastFiredAt, now)}</span
								>{:else}never fired{/if}
						{:else if t.lastFiredAt}
							Fired <span title={fmtTime(t.lastFiredAt)}>{fmtAgo(t.lastFiredAt, now)}</span>
							{#if h?.count}· {countLine(h)}{:else if t.fireCount}· {t.fireCount} action{t.fireCount ===
								1
									? ''
									: 's'} so far{/if}
						{:else}
							Never fired{#if needs(t.kind)}
								· {needs(t.kind)}{/if}
						{/if}
					</div>
				</div>
				{#if admin}
					<RowMenu label="Actions for {t.name}">
						<button
							type="button"
							class="menu-item"
							role="menuitem"
							disabled={dryBusy}
							onclick={() => dryRun(t.kind, t.config, t.id)}
							>{t.kind === 'restart_notice' ? 'Preview next cycle' : 'Dry run, last 24 h'}</button
						>
						<button type="button" class="menu-item" role="menuitem" onclick={() => open(t.kind, t)}
							>Edit</button
						>
						<button
							type="button"
							class="menu-item"
							role="menuitem"
							onclick={() => open(t.kind, t, true)}>Duplicate</button
						>
						<hr class="my-1 border-black" />
						<button
							type="button"
							class="menu-item text-danger!"
							role="menuitem"
							disabled={busy}
							onclick={() => remove(t)}>Delete</button
						>
					</RowMenu>
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
				Add a rule to start.{/if}
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
			<p class="-mt-2 text-[13px] text-mist-400">{blurb(f.kind)}</p>
			{#if needs(f.kind)}<p class="note mb-0 text-warn">{needs(f.kind)}</p>{/if}
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
			{:else if f.kind === 'risk_kick'}
				<div class="space-y-1.5 text-[13px]">
					<label class="flex items-center gap-2"
						><input type="checkbox" bind:checked={f.bannedElsewhere} /> Banned on another server in this
						organisation</label
					>
					<label class="flex items-center gap-2"
						><input type="checkbox" bind:checked={f.watchlist} /> On the watchlist</label
					>
					<label class="flex items-center gap-2 {data.steam ? '' : 'text-mist-600'}"
						><input type="checkbox" bind:checked={f.vacBans} disabled={!data.steam} /> Any VAC ban on
						record</label
					>
					<label class="flex items-center gap-2 {data.steam ? '' : 'text-mist-600'}"
						><input type="checkbox" bind:checked={f.gameBans} disabled={!data.steam} /> Any game ban on
						record</label
					>
					<div class="flex flex-wrap items-center gap-2 {data.steam ? '' : 'text-mist-600'}">
						Steam account younger than
						<input
							class="input w-20 text-right"
							type="number"
							min="0"
							max="3650"
							bind:value={f.minAccountDays}
							disabled={!data.steam}
						/>
						days (0 = off)
					</div>
					<label class="flex items-center gap-2 pl-5 {data.steam ? '' : 'text-mist-600'}"
						><input
							type="checkbox"
							bind:checked={f.privateProfiles}
							disabled={!data.steam || !f.minAccountDays}
						/> …and treat private profiles (age unknown) as too young</label
					>
					<label class="flex flex-wrap items-center gap-2"
						>Or at the advisory risk level
						<select class="input w-auto pr-[30px]" bind:value={f.kickAtLevel}>
							<option value="">off</option>
							<option value="high">high</option>
							<option value="medium">medium or high</option>
						</select>
						as the players table shows it</label
					>
					<label class="flex items-center gap-2"
						><input type="checkbox" bind:checked={f.spareReserved} /> Never kick players with a reserved
						slot</label
					>
				</div>
				<label class="block"
					><span class="field-label">Kick reason shown to the player</span><input
						class="input"
						type="text"
						bind:value={f.reason}
						maxlength="200"
					/></label
				>
				{#if data.steam}
					<p class="note">
						Steam data is fetched when a player first appears and refreshed daily. Kicks land in the
						audit trail with the rule that matched.
					</p>
				{/if}
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
			{:else if f.kind === 'seed_reward'}
				<div class="grid grid-cols-2 gap-3">
					<label class="block"
						><span class="field-label">Counts as seeding: at most (players on)</span><input
							class="input"
							type="number"
							min="1"
							max="1000"
							bind:value={f.lowAt}
							required
						/></label
					>
					<label class="block"
						><span class="field-label">Seed time needed (minutes)</span><input
							class="input"
							type="number"
							min="1"
							max="129600"
							bind:value={f.minutes}
							required
						/></label
					>
					<label class="block"
						><span class="field-label">Counted over the last (days)</span><input
							class="input"
							type="number"
							min="1"
							max="90"
							bind:value={f.windowDays}
							required
						/></label
					>
					<label class="block"
						><span class="field-label">Reserved slot lasts (days)</span><input
							class="input"
							type="number"
							min="1"
							max="365"
							bind:value={f.slotDays}
							required
						/></label
					>
				</div>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={f.untilFull} /> Only count seeding once the server has
					filled with the player still on</label
				>
				<label class="block"
					><span class="field-label"
						>Filled means at least (players; blank for the server's limit)</span
					><input
						class="input"
						type="number"
						min="1"
						max="1000"
						bind:value={f.fullAt}
						disabled={!f.untilFull}
					/></label
				>
				<label class="block"
					><span class="field-label">Whisper on the grant (blank for none)</span><input
						class="input"
						type="text"
						bind:value={f.message}
						maxlength="200"
					/></label
				>
				<p class="note">
					Every minute a player is on with that many or fewer players counts as seed time. With the
					box ticked it stays pending until the server has filled (the number above, or the player
					limit the server reports) with the player still on; leave before that and it is forfeited,
					so staying until the threshold and going, or a few minutes on an empty server, earns
					nothing. Unticked, every low minute counts as it passes. When banked seed time reaches the
					target within the window, the player goes on the organisation's reserved-slot list with
					that expiry: this server applies it at once, the organisation's other servers at their
					next sync, and it can be earned again once it lapses. Players who already hold a reserved
					slot are skipped. Placeholders: <span class="chip">{'{name}'}</span>
					<span class="chip">{'{server}'}</span>
					<span class="chip">{'{minutes}'}</span> <span class="chip">{'{until}'}</span>
					<span class="chip">{'{days}'}</span> <span class="chip">{'{players}'}</span>
					<span class="chip">{'{max}'}</span>.
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
