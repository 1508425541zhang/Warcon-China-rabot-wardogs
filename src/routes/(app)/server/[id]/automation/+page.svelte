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
	/** Narrow the table to one rule (by name, so a deleted rule's rows still group) or one state. */
	let ruleFilter = $state('');
	let stateFilter = $state<'' | OutboxView['state']>('');
	const STATES: OutboxView['state'][] = [
		'delivered',
		'failed',
		'skipped',
		'unknown',
		'pending',
		'sending'
	];
	let ruleNames = $derived([...new Set(deliveries.map((d) => d.triggerName))].sort());
	let deliveryRows = $derived(
		deliverySort.sorted(
			deliveries.filter(
				(d) =>
					(!ruleFilter || d.triggerName === ruleFilter) &&
					(!stateFilter || d.state === stateFilter) &&
					matches(deliverySearch, d.triggerName, d.action, d.target, d.state, d.outcome)
			)
		)
	);
	let actionsPanel = $state<HTMLElement>();
	/** From a failing rule's row to its deliveries: set the rule filter and bring the table up. */
	function seeActions(t: TriggerView) {
		ruleFilter = t.name;
		stateFilter = '';
		deliverySearch = '';
		actionsPanel?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}
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
			kind: 'match_broadcast',
			group: 'Messages',
			label: 'Match broadcast',
			blurb: 'Announce who won when a match ends, and the map as the next one starts.'
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
		endMessage: string;
		startMessage: string;
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
	/**
	 * Placeholder chips insert into the message field the admin last had the caret in, or the first
	 * one in the form; typing `{faction}` by hand is the commonest thing to get wrong.
	 */
	let formEl = $state<HTMLFormElement>();
	let lastField: HTMLInputElement | HTMLTextAreaElement | null = null;
	const isText = (el: unknown): el is HTMLInputElement | HTMLTextAreaElement =>
		el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && el.type === 'text');
	function insert(token: string) {
		const el =
			lastField?.isConnected && !lastField.disabled
				? lastField
				: formEl?.querySelector<HTMLInputElement | HTMLTextAreaElement>(
						'textarea:not([disabled]), input[type=text]:not([disabled]):not([name=name])'
					);
		if (!el) return;
		const at = el.selectionStart ?? el.value.length;
		el.setRangeText(`{${token}}`, at, el.selectionEnd ?? at, 'end');
		el.dispatchEvent(new Event('input', { bubbles: true }));
		el.focus();
	}
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
			endMessage: s('endMessage', 'Match over: {faction} wins on {previous} · {scores}'),
			startMessage: s('startMessage', 'New match on {map}. Good luck!'),
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
			case 'match_broadcast':
				return {
					endMessage: f.endMessage,
					startMessage: f.startMessage,
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
		if (await run(() => api('DELETE', `${path}/${t.id}`), 'Trigger deleted.'))
			if (dryFor === t.id) dry = null;
	}
	/** The result panel is titled with the rule it was run for; 'form' keys a run from the editor. */
	let dryTitle = $state('');
	async function dryRun(
		kind: TriggerKind,
		cfg: Record<string, unknown>,
		key: string,
		title: string
	) {
		dryBusy = true;
		dryFor = key;
		dryTitle = title;
		try {
			dry = (await api<{ result: DryRunResult }>('POST', `${path}/dry-run`, { kind, config: cfg }))
				.result;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			dryBusy = false;
		}
	}

	/** A dry run's lines with repeats folded: a broadcast replayed 96 times is one line, ×96. */
	function grouped(items: DryRunResult['items']): { at: string; text: string; n: number }[] {
		const out: { at: string; text: string; n: number }[] = [];
		for (const it of items) {
			const last = out[out.length - 1];
			if (last && last.text === it.text) last.n++;
			else out.push({ at: it.at, text: it.text, n: 1 });
		}
		return out;
	}

	/** The rule as one sentence; the list shows it, and the editor shows it live as "Reads as". */
	function describe(kind: TriggerKind, config: Record<string, unknown>): string {
		const c = config;
		switch (kind) {
			case 'welcome':
				return `"${c.message}"${c.afterFaction ? ' · after faction pick' : ' · on join'}${c.onlyFirstVisit ? ' · first visit only' : ''}`;
			case 'faction_change':
				return `"${c.message}"`;
			case 'broadcast':
				return `${(c.messages as string[]).length} message${(c.messages as string[]).length === 1 ? '' : 's'} every ${c.everyMinutes} min · ${typeof c.maxPlayers === 'number' ? `${c.minPlayers} to ${c.maxPlayers}` : `at least ${c.minPlayers}`} on`;
			case 'empty_reset':
				return `to ${c.map ? mapLabel(data.catalog, String(c.map)) : 'the chosen map'} after ${c.afterMinutes} min empty`;
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
			case 'match_broadcast':
				return [
					c.endMessage ? `end: "${c.endMessage}"` : '',
					c.startMessage ? `start: "${c.startMessage}"` : ''
				]
					.filter(Boolean)
					.join(' · ')
					.concat(` · at least ${c.minPlayers} on`);
			case 'team_kill':
				return [
					c.warnAt ? `whisper from ${c.warnAt} team kill${c.warnAt === 1 ? '' : 's'}` : '',
					c.kickAt ? `kick at ${c.kickAt}` : ''
				]
					.filter(Boolean)
					.join(' · ')
					.concat(' · per session');
			case 'seed_reward':
				return `${c.minutes} min with ${c.lowAt} or fewer on${c.untilFull === false ? '' : `, staying until ${typeof c.fullAt === 'number' ? `${c.fullAt}+ on` : 'it fills'}`}, within ${c.windowDays} day${c.windowDays === 1 ? '' : 's'} · slot for ${c.slotDays} day${c.slotDays === 1 ? '' : 's'}${c.message ? ' · with a whisper' : ''}`;
		}
	}
</script>

<svelte:window
	onclick={() => (addOpen = false)}
	onkeydown={(e) => e.key === 'Escape' && (addOpen = false)}
/>

<p class="mb-4 text-[13px] text-mist-400">
	Rules act on {data.server.demo ? 'the demo server' : 'this server'} as things happen: a join, a kill,
	a quiet hour. Every action is recorded below and in the audit trail as
	<span class="chip">trigger</span>.
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

<!-- A dry run from a row opens here, above the list, so the rows never change height. -->
{#if dry && dryFor !== 'form' && !form}
	<div class="mb-3 rounded-ctl border border-l-2 border-black border-l-accent bg-ink-900 p-3">
		{@render dryResult(dry, dryTitle)}
	</div>
{/if}

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
					<div class="mt-0.5 line-clamp-2 text-[13px] text-mist-400">
						{describe(t.kind, t.config)}
					</div>
					<!-- One of four shapes, most urgent first: failing, off, fired, never fired. -->
					<div class="mt-0.5 text-[12px] {h?.failing ? 'text-mist-100' : 'text-mist-600'}">
						{#if h?.failing}
							Latest actions failed · <span class="font-mono text-[11.5px] text-mist-400"
								>{h.outcome}</span
							>
							· <span title={fmtTime(h.latest)}>{fmtAgo(h.latest, now)}</span>
							<button type="button" class="ml-1 btn btn-sm" onclick={() => seeActions(t)}
								>See actions</button
							>
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
							onclick={() => dryRun(t.kind, t.config, t.id, t.name)}
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
		</div>
	{:else}
		<div class="flex flex-col items-center gap-3 panel py-7 text-center">
			{#if admin && !onlyFailing}
				<p class="text-mist-100">Most servers start with these two.</p>
				<div class="flex flex-wrap justify-center gap-2">
					<button type="button" class="btn btn-primary" onclick={() => open('welcome')}
						>+ Welcome whisper</button
					>
					<button type="button" class="btn" onclick={() => open('broadcast')}
						>+ Scheduled broadcast</button
					>
				</div>
				<p class="max-w-[52ch] text-[12.5px] text-mist-600">
					Or add any rule: {KINDS.filter((k) => k.kind !== 'welcome' && k.kind !== 'broadcast')
						.map((k) => k.label)
						.join(' · ')}.
				</p>
			{:else if onlyFailing}
				<p class="text-mist-600">No rule is failing.</p>
			{:else}
				<p class="text-mist-600">No rules on this server yet.</p>
			{/if}
		</div>
	{/each}
</div>

{#snippet placeholders(names: string[])}
	<div class="flex flex-wrap items-center gap-1 text-[12px] text-mist-600">
		<span class="mr-1">Insert</span>
		{#each names as n (n)}
			<button
				type="button"
				class="chip cursor-pointer text-mist-100 transition hover:bg-white/12"
				title="Insert {'{' + n + '}'} at the caret"
				onclick={() => insert(n)}>{'{' + n + '}'}</button
			>
		{/each}
	</div>
{/snippet}

{#snippet dryResult(r: DryRunResult, title: string)}
	<div class="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px]">
		<span class="caps text-accent"
			>{r.kind === 'restart_notice' ? 'Next cycle' : 'Dry run'} · {title}</span
		>
		{#if r.kind === 'restart_notice'}
			<span
				><b class={r.fires ? 'text-warn' : 'text-ok'}>{r.fires}</b> broadcast{r.fires === 1
					? ''
					: 's'}</span
			>
		{:else}
			<span
				>Replayed the last 24 h on this server: would have fired <b
					class={r.fires ? 'text-warn' : 'text-ok'}>{r.fires}</b
				>
				time{r.fires === 1 ? '' : 's'}</span
			>
		{/if}
		<button
			type="button"
			class="ml-auto btn btn-sm btn-ghost"
			aria-label="Close the dry run"
			onclick={() => (dry = null)}>✕</button
		>
	</div>
	{#if r.items.length}
		<ul class="max-h-56 space-y-0.5 overflow-y-auto font-mono text-[12px]">
			{#each grouped(r.items) as it, i (i)}
				<li>
					<span class="text-mist-600">{fmtTime(it.at)}</span>
					{it.text}
					{#if it.n > 1}<span class="text-mist-600">×{it.n}</span>{/if}
				</li>
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
			bind:this={formEl}
			onfocusin={(e) => {
				if (isText(e.target) && e.target.name !== 'name') lastField = e.target;
			}}
			onsubmit={(e) => {
				e.preventDefault();
				save();
			}}
		>
			<p class="-mt-2 text-[13px] text-mist-400">{blurb(f.kind)}</p>
			{#if needs(f.kind)}<p class="note mb-0 text-warn">{needs(f.kind)}</p>{/if}
			<!-- Hidden, not unmounted, while the dry run shows: the map picker keeps its choice. -->
			<div class="space-y-3" class:hidden={dry && dryFor === 'form'}>
				<div class="grid grid-cols-1 gap-3 sm:grid-cols-[1fr_auto]">
					<label class="block"
						><span class="field-label">Name</span><input
							class="input"
							type="text"
							name="name"
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
					<fieldset class="space-y-2">
						<legend class="field-label">Whisper</legend>
						<input class="input" type="text" bind:value={f.message} maxlength="200" required />
						{@render placeholders(['name', 'faction', 'server', 'map', 'players', 'max'])}
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">When</legend>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.afterFaction} /> Wait until the player has picked
							a faction</label
						>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.onlyFirstVisit} /> Only on a player's first visit
							to this server</label
						>
					</fieldset>
					<p class="note">Sent as a whisper, so only that player sees it.</p>
				{:else if f.kind === 'faction_change'}
					<fieldset class="space-y-2">
						<legend class="field-label">Whisper</legend>
						<input class="input" type="text" bind:value={f.message} maxlength="200" required />
						{@render placeholders([
							'name',
							'faction',
							'previous',
							'server',
							'map',
							'players',
							'max'
						])}
					</fieldset>
					<p class="note">
						Fires when a player moves from one faction to another, not on their first pick after
						joining.
					</p>
				{:else if f.kind === 'broadcast'}
					<fieldset class="space-y-2">
						<legend class="field-label">Messages, one per line, sent in turn</legend>
						<textarea class="min-h-[100px] input" bind:value={f.messages} required></textarea>
						{@render placeholders(['server', 'map', 'players', 'max'])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">When</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							Every
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.everyMinutes}
								aria-label="Every, minutes"
								required
							/>
							min, with at least
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.minPlayers}
								aria-label="At least, players"
							/>
							and at most
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.maxPlayers}
								aria-label="At most, players"
								placeholder="any"
							/>
							players on
						</div>
					</fieldset>
					<p class="note">
						Blank for no ceiling; a fill-the-server message can stop once it has. Broadcasts are
						limited to 200 characters.
					</p>
				{:else if f.kind === 'empty_reset'}
					<fieldset class="space-y-2">
						<legend class="field-label">Reset to</legend>
						<MapPicker bind:this={picker} serverId={id} catalog={data.catalog} />
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">When</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							After the server has been empty for
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.afterMinutes}
								aria-label="After empty for, minutes"
								required
							/>
							min, at most once every
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.cooldownMinutes}
								aria-label="Cooldown between resets, minutes"
							/>
							min
						</div>
					</fieldset>
					<p class="note">
						Fires when nobody has been on for that long and the server is on a different map or
						mode. With a rotation the target is set as next and the match ended; without one the map
						is requested directly.
					</p>
				{:else if f.kind === 'restart_notice'}
					<fieldset class="space-y-2">
						<legend class="field-label">Heads-up, before the window opens</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							Send
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="719"
								bind:value={f.leadMinutes}
								aria-label="Heads-up, minutes before"
							/>
							min before <span class="text-mist-600">(0 turns the heads-up off)</span>
						</div>
						<input
							class="input"
							type="text"
							bind:value={f.leadMessage}
							maxlength="200"
							aria-label="Heads-up message"
							disabled={!Number(f.leadMinutes)}
						/>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">Once the window is open</legend>
						<input
							class="input"
							type="text"
							bind:value={f.message}
							maxlength="200"
							aria-label="Message once the window is open"
							required
						/>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							Repeat every
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1440"
								bind:value={f.repeatMinutes}
								aria-label="Repeat every, minutes"
							/>
							min while the round runs on <span class="text-mist-600">(0 sends it once)</span>
						</div>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">Only with at least</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.minPlayers}
								aria-label="At least, players"
							/>
							players on
						</div>
					</fieldset>
					{@render placeholders(['minutes', 'uptime', 'server', 'map', 'players', 'max'])}
					<p class="note">
						The game restarts twelve hours after it started, once the round then in progress ends.
					</p>
				{:else if f.kind === 'match_broadcast'}
					<fieldset class="space-y-2">
						<legend class="field-label">When a match ends</legend>
						<input
							class="input"
							type="text"
							bind:value={f.endMessage}
							maxlength="200"
							aria-label="Message when a match ends"
							placeholder="Leave empty to say nothing"
						/>
						{@render placeholders([
							'faction',
							'score',
							'scores',
							'cap',
							'previous',
							'map',
							'server',
							'players'
						])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">As the next one starts</legend>
						<input
							class="input"
							type="text"
							bind:value={f.startMessage}
							maxlength="200"
							aria-label="Message as the next match starts"
							placeholder="Leave empty to say nothing"
						/>
						{@render placeholders(['map', 'previous', 'server', 'players', 'max'])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">Only with at least</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.minPlayers}
								aria-label="At least, players"
							/>
							players on
						</div>
					</fieldset>
					<p class="note">
						A match ends when the map changes or the faction scores fall back to zero, so a manual
						end or map change counts too; {'{faction}'} is whoever led at that moment (tied factions are
						named together). Sent one poll after the round ends, a second or two on a busy server and
						up to half a minute on a quiet one.
					</p>
				{:else if f.kind === 'risk_kick'}
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">Kick when the player is</legend>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.bannedElsewhere} /> banned on another server in
							this organisation</label
						>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.watchlist} /> on the watchlist</label
						>
						<div
							class="grid grid-cols-1 gap-x-4 gap-y-1.5 border-t border-black pt-2 sm:grid-cols-[1fr_auto] {data.steam
								? ''
								: 'text-mist-600'}"
						>
							<div class="space-y-1.5">
								<label class="flex items-center gap-2"
									><input type="checkbox" bind:checked={f.vacBans} disabled={!data.steam} /> VAC banned</label
								>
								<label class="flex items-center gap-2"
									><input type="checkbox" bind:checked={f.gameBans} disabled={!data.steam} /> game banned</label
								>
								<div class="flex flex-wrap items-center gap-2">
									under
									<input
										class="input w-20 text-right"
										type="number"
										min="0"
										max="3650"
										bind:value={f.minAccountDays}
										aria-label="Steam account younger than, days"
										disabled={!data.steam}
									/>
									days old <span class="text-mist-600">(0 turns it off)</span>
								</div>
								<label class="flex items-center gap-2 pl-5"
									><input
										type="checkbox"
										bind:checked={f.privateProfiles}
										disabled={!data.steam || !f.minAccountDays}
									/> and treat private profiles, whose age is unknown, as too young</label
								>
							</div>
							<p
								class="max-w-[22ch] text-[12px] text-mist-600 sm:border-l sm:border-ink-700 sm:pl-3"
							>
								From Steam, fetched when a player first appears and refreshed daily.
							</p>
						</div>
						<label class="flex flex-wrap items-center gap-2 border-t border-black pt-2"
							>at advisory risk level
							<select class="input w-auto pr-[30px]" bind:value={f.kickAtLevel}>
								<option value="">off</option>
								<option value="high">high</option>
								<option value="medium">medium or high</option>
							</select>
							as the players table shows it</label
						>
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">Never kick</legend>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.spareReserved} /> players with a reserved slot</label
						>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">Kick reason, shown to the player</legend>
						<input class="input" type="text" bind:value={f.reason} maxlength="200" />
					</fieldset>
					<p class="note">Kicks land in the audit trail with the rule that matched.</p>
				{:else if f.kind === 'team_kill'}
					<fieldset class="space-y-2">
						<legend class="field-label">Whisper</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							From
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="100"
								bind:value={f.warnAt}
								aria-label="Whisper from, team kills"
							/>
							team kills, on every one after <span class="text-mist-600">(0 turns it off)</span>
						</div>
						<input
							class="input"
							type="text"
							bind:value={f.warnMessage}
							maxlength="200"
							aria-label="Whisper"
							disabled={!Number(f.warnAt)}
						/>
						{@render placeholders(['name', 'victim', 'count', 'server', 'map'])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">Kick</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							At
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="100"
								bind:value={f.kickAt}
								aria-label="Kick at, team kills"
							/>
							team kills <span class="text-mist-600">(0 turns it off)</span>
						</div>
						<input
							class="input"
							type="text"
							bind:value={f.kickReason}
							maxlength="200"
							aria-label="Kick reason"
							disabled={!Number(f.kickAt)}
						/>
					</fieldset>
					<p class="note">
						Team kills come from the game's kill feed and are counted per player within their
						current session.
					</p>
				{:else if f.kind === 'seed_reward'}
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">Counts as seeding</legend>
						<div class="flex flex-wrap items-center gap-2">
							Being on with at most
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1000"
								bind:value={f.lowAt}
								aria-label="Counts as seeding: at most, players on"
								required
							/>
							players on
						</div>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.untilFull} /> and only once the server has filled
							with the player still on</label
						>
						<div
							class="flex flex-wrap items-center gap-2 pl-5 {f.untilFull ? '' : 'text-mist-600'}"
						>
							Filled means at least
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1000"
								bind:value={f.fullAt}
								aria-label="Filled means at least, players"
								placeholder="limit"
								disabled={!f.untilFull}
							/>
							players <span class="text-mist-600">(blank for the server's own limit)</span>
						</div>
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">Reward</legend>
						<div class="flex flex-wrap items-center gap-2">
							<input
								class="input w-24 text-right"
								type="number"
								min="1"
								max="129600"
								bind:value={f.minutes}
								aria-label="Seed time needed, minutes"
								required
							/>
							min of seed time within the last
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="90"
								bind:value={f.windowDays}
								aria-label="Counted over the last, days"
								required
							/>
							days earns a reserved slot for
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="365"
								bind:value={f.slotDays}
								aria-label="Reserved slot lasts, days"
								required
							/>
							days
						</div>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">Whisper on the grant, blank for none</legend>
						<input class="input" type="text" bind:value={f.message} maxlength="200" />
						{@render placeholders(['name', 'server', 'minutes', 'until', 'days', 'players', 'max'])}
					</fieldset>
					<p class="note">
						With the box ticked, seed time stays pending until the server has filled with the player
						still on; leave before that and it is forfeited. Unticked, every low minute counts as it
						passes. The slot goes on the organisation's reserved-slot list: this server applies it
						at once, the other servers at their next sync, and it can be earned again once it
						lapses. Players who already hold a reserved slot are skipped.
					</p>
				{/if}

				<div class="rounded-ctl border border-black bg-ink-950 px-3 py-2 text-[13px]">
					<span class="mr-2 caps text-accent">Reads as</span>
					<span class="text-mist-100">{describe(f.kind, config(f))}</span>
				</div>
			</div>

			{#if dry && dryFor === 'form'}
				<div class="rounded-ctl border border-black bg-ink-950 p-3">
					{@render dryResult(dry, f.name)}
				</div>
			{/if}

			<div class="flex flex-wrap justify-end gap-2 pt-2">
				<button
					type="button"
					class="mr-auto btn"
					disabled={dryBusy}
					onclick={() =>
						dry && dryFor === 'form' ? (dry = null) : dryRun(f.kind, config(f), 'form', f.name)}
					>{dryBusy
						? 'Working…'
						: dry && dryFor === 'form'
							? 'Back to the form'
							: f.kind === 'restart_notice'
								? 'Preview next cycle'
								: 'Dry run, last 24 h'}</button
				>
				<button type="button" class="btn" data-close onclick={() => (form = null)}>Cancel</button>
				<button type="submit" class="btn btn-primary" disabled={busy}
					>{f.id ? 'Save' : 'Add rule'}</button
				>
			</div>
		</form>
	</Modal>
{/if}

{#if data.triggers.length || deliveries.length}
	<div class="mt-4 scroll-mt-4 panel" bind:this={actionsPanel}>
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<span class="label-sm mb-0">Recent actions</span>
			<span class="text-[12.5px] text-mist-600">what the rules did, newest first</span>
			<div class="flex w-full flex-wrap gap-2 sm:ml-auto sm:w-auto">
				<select
					class="input w-auto pr-[30px] {ruleFilter ? 'border-accent' : ''}"
					aria-label="Only this rule"
					bind:value={ruleFilter}
				>
					<option value="">All rules</option>
					{#each ruleNames as name (name)}<option value={name}>{name}</option>{/each}
				</select>
				<select
					class="input w-auto pr-[30px] {stateFilter ? 'border-accent' : ''}"
					aria-label="Only this state"
					bind:value={stateFilter}
				>
					<option value="">Any state</option>
					{#each STATES as s (s)}<option value={s}>{s}</option>{/each}
				</select>
				<input
					class="input w-full sm:w-52"
					type="search"
					placeholder="Filter by action, target, result…"
					aria-label="Filter recent actions"
					bind:value={deliverySearch}
				/>
			</div>
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
							<td
								><Badge
									tone={stateTone(d.state)}
									title={d.state === 'unknown'
										? 'Sent, no answer from the game. Not retried.'
										: undefined}>{d.state}</Badge
								></td
							>
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
{/if}
