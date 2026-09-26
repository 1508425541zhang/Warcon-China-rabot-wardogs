<script lang="ts">
	import SkillBalanceSettings from '$lib/components/SkillBalanceSettings.svelte';
	import FactionLockSettings from '$lib/components/FactionLockSettings.svelte';
	import WeaponRestrictionSettings from '$lib/components/WeaponRestrictionSettings.svelte';
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtAgo, fmtSpan, fmtTime, mapLabel } from '$lib/format';
	import { can } from '$lib/capabilities';
	import { isSteamId } from '$lib/steam-profiles';
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

	/** A risk_kick rule's score threshold, 0 when off; rules saved with a level read as 20 or 50. */
	const kickAtScoreOf = (c: Record<string, unknown>): number =>
		typeof c.kickAtScore === 'number'
			? c.kickAtScore
			: c.kickAtLevel === 'high'
				? 50
				: c.kickAtLevel === 'medium'
					? 20
					: 0;

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
			label: '欢迎私信',
			blurb: '玩家加入或选择阵营后发送私信。'
		},
		{
			kind: 'faction_change',
			group: 'Messages',
			label: '切换阵营私信',
			blurb: '玩家切换阵营时发送私信。'
		},
		{
			kind: 'broadcast',
			group: 'Messages',
			label: '定时广播',
			blurb: '服务器有玩家在线时，按设定间隔轮流广播消息。'
		},
		{
			kind: 'restart_notice',
			group: 'Messages',
			label: '重启通知',
			blurb: '在每日重启前提醒玩家，并在重启时通知。'
		},
		{
			kind: 'match_broadcast',
			group: 'Messages',
			label: '对局广播',
			blurb: '对局结束时播报胜方，下一局开始时播报地图。'
		},
		{
			kind: 'risk_kick',
			group: 'Players',
			label: '入服风险踢出',
			blurb: '按账号历史和观察名单判断入服风险；与社区行为风控评分相互独立。'
		},
		{
			kind: 'name_filter',
			group: 'Players',
			label: '昵称过滤',
			blurb: '对昵称包含禁用字符或词语的玩家踢出或标记。'
		},
		{
			kind: 'ping_kick',
			group: 'Players',
			label: '高延迟踢出',
			blurb: '玩家延迟持续超过设定阈值时踢出。'
		},
		{
			kind: 'team_kill',
			group: 'Players',
			label: '队友击杀限制',
			blurb: '队友击杀达到警告值时发送私信，超过上限时踢出。'
		},
		{
			kind: 'kill_rate',
			group: 'Players',
			label: '击杀速率观察',
			blurb: '击杀过快或爆头率过高时标记玩家，供管理员复核。'
		},
		{
			kind: 'seed_reward',
			group: 'Players',
			label: '种子玩家奖励',
			blurb: '为低活跃时期留在服务器的玩家发放预留席位。'
		},
		{
			kind: 'empty_reset',
			group: 'Server',
			label: '空服地图重置',
			blurb: '服务器空置一段时间后切换回指定地图。'
		}
	];
	const GROUPS: Group[] = ['Messages', 'Players', 'Server'];
	const groupLabel: Record<Group, string> = {
		Messages: '消息',
		Players: '玩家',
		Server: '服务器'
	};
	/** The outbox action as the table shows it: a flag sends nothing to the game, so it reads as one. */
	const actionLabel = (action: string) =>
		({
			name_flag: '标记',
			kill_rate_flag: '标记',
			kick: '踢出',
			whisper: '私信',
			broadcast: '广播',
			map_change: '切换地图',
			reserve: '预留席位'
		})[action as 'name_flag'] ?? action;
	const deliveryStateLabel = (state: OutboxView['state']) =>
		({
			delivered: '已送达',
			failed: '失败',
			skipped: '已跳过',
			unknown: '结果未知',
			pending: '等待中',
			sending: '发送中'
		})[state];
	const label = (kind: TriggerKind) => KINDS.find((k) => k.kind === kind)?.label ?? kind;
	const blurb = (kind: TriggerKind) => KINDS.find((k) => k.kind === kind)?.blurb ?? '';
	/** Why a kind cannot run on this server yet, or '' when it can. */
	let needs = $derived((kind: TriggerKind): string => {
		switch (kind) {
			case 'team_kill':
			case 'kill_rate':
				return data.feed ? '' : '需要击杀事件源。请先在“配置”中为本服务器启用。';
			case 'risk_kick':
				return data.steam ? '' : '面板未启用 Steam 查询；目前只能使用封禁列表与关注列表条件。';
			case 'seed_reward':
				return canSlotHere || canSlotOrg
					? ''
					: '保存此规则需要“自动化”和“预留席位”权限；跨服席位还需要“组织预留席位”权限。';
			default:
				return '';
		}
	});
	/**
	 * what a Seeding reward may hand out: a slot on this server (Reserved slots) or on every server
	 * (Org reserved slots)
	 */
	let canSlotHere = $derived(can(data.server.caps, 'slots.manage'));
	let canSlotOrg = $derived(can(data.server.caps, 'lists.reserve'));
	/** A kind that lacks what it needs stays in the menu, greyed, with the reason in a few words. */
	const short = (kind: TriggerKind): string =>
		kind === 'team_kill' || kind === 'kill_rate'
			? '需要击杀事件源'
			: kind === 'risk_kick'
				? '需要 Steam 密钥'
				: '';
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
		coversToday ? `今天 ${h.count} 次` : `近 ${fmtSpan(now - windowStart)} 内 ${h.count} 次`;
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
		maxPlayers: number | null;
		afterMinutes: number;
		cooldownMinutes: number;
		vacBans: boolean;
		gameBans: boolean;
		maxBanAgeDays: number;
		maxPingMs: number;
		durationSeconds: number;
		minAccountDays: number;
		privateProfiles: boolean;
		bannedElsewhere: boolean;
		watchlist: boolean;
		kickAtScore: number;
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
		fullAt: number | null;
		minutes: number;
		windowDays: number;
		slotDays: number;
		slotScope: 'server' | 'org';
		characters: 'off' | 'latin' | 'ascii';
		extraScripts: string[];
		allowSymbols: boolean;
		minLetters: number;
		builtinWords: boolean;
		blocked: string;
		allowed: string;
		nameAction: 'kick' | 'alert';
		windowMinutes: number;
		maxKills: number;
		headshotPct: number;
		headshotMinKills: number;
	}
	/** The alphabets a Latin policy can let in, by the name the rule stores and the one people use. */
	const SCRIPTS: [string, string][] = [
		['Cyrillic', 'Cyrillic'],
		['Greek', 'Greek'],
		['Arabic', 'Arabic'],
		['Hebrew', 'Hebrew'],
		['Thai', 'Thai'],
		['Devanagari', 'Devanagari'],
		['Han', 'Chinese'],
		['Hiragana', 'Hiragana'],
		['Katakana', 'Katakana'],
		['Hangul', 'Korean']
	];
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
						'textarea:not([disabled]):not([data-plain]), input[type=text]:not([disabled]):not([name=name])'
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
					? '{name}，你现在属于 {faction} 阵营。'
					: kind === 'restart_notice'
						? '计划重启：本局结束后服务器将重启，请稍后重新加入。'
						: kind === 'seed_reward'
							? '感谢 {name} 为 {server} 暖服！你的预留席位有效至 {until}。'
							: '欢迎 {name} 来到 {server}！输入 /rules 查看规则。'
			),
			onlyFirstVisit: b('onlyFirstVisit', false),
			afterFaction: b('afterFaction', false),
			messages: Array.isArray(c.messages)
				? (c.messages as string[]).join('\n')
				: '加入我们的 Discord，获取活动信息和帮助。\n禁止队友击杀，管理员会持续巡查。',
			everyMinutes: n('everyMinutes', 15),
			minPlayers: n('minPlayers', 1),
			maxPlayers: typeof c.maxPlayers === 'number' ? c.maxPlayers : null,
			afterMinutes: n('afterMinutes', 20),
			cooldownMinutes: n('cooldownMinutes', 30),
			vacBans: b('vacBans', true),
			gameBans: b('gameBans', false),
			maxBanAgeDays: n('maxBanAgeDays', 0),
			maxPingMs: n('maxPingMs', 200),
			durationSeconds: n('durationSeconds', 60),
			minAccountDays: n('minAccountDays', 0),
			privateProfiles: b('privateProfiles', false),
			bannedElsewhere: b('bannedElsewhere', true),
			watchlist: b('watchlist', false),
			kickAtScore: kickAtScoreOf(c),
			spareReserved: b('spareReserved', true),
			reason: s(
				'reason',
				kind === 'name_filter'
					? '你的昵称不符合本服务器规则：{why}。'
					: kind === 'ping_kick'
						? '延迟持续过高。'
						: '你的账号不符合本服务器的入服要求。'
			),
			leadMinutes: n('leadMinutes', 30),
			leadMessage: s('leadMessage', '服务器将在约 {minutes} 分钟后于本局结束时重启。'),
			repeatMinutes: n('repeatMinutes', 0),
			endMessage: s('endMessage', '对局结束：{faction} 在 {previous} 获胜 · {scores}'),
			startMessage: s('startMessage', '新对局地图：{map}。祝大家玩得开心！'),
			warnAt: n('warnAt', 2),
			warnMessage: s('warnMessage', '{name}，请注意避免队友击杀（本次会话已发生 {count} 次）。'),
			kickAt: n('kickAt', 4),
			kickReason: s('kickReason', '队友击杀（本次会话 {count} 次）。'),
			lowAt: n('lowAt', 20),
			untilFull: b('untilFull', true),
			fullAt: typeof c.fullAt === 'number' ? c.fullAt : null,
			minutes: n('minutes', 60),
			windowDays: n('windowDays', 7),
			slotDays: n('slotDays', 7),
			// a rule saved before the scope existed hands out org-wide slots; a new one, this server's
			slotScope: c.scope === 'server' ? 'server' : t ? 'org' : canSlotHere ? 'server' : 'org',
			characters: c.characters === 'ascii' || c.characters === 'off' ? c.characters : 'latin',
			extraScripts: Array.isArray(c.extraScripts) ? (c.extraScripts as string[]) : [],
			allowSymbols: b('allowSymbols', false),
			minLetters: n('minLetters', 0),
			builtinWords: b('builtinWords', !t),
			blocked: Array.isArray(c.blocked) ? (c.blocked as string[]).join('\n') : '',
			allowed: Array.isArray(c.allowed) ? (c.allowed as string[]).join('\n') : '',
			nameAction: c.action === 'alert' ? 'alert' : 'kick',
			windowMinutes: n('windowMinutes', 5),
			maxKills: n('maxKills', 25),
			headshotPct: n('headshotPct', 70),
			headshotMinKills: n('headshotMinKills', 15)
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

	const dryLabel = (kind: TriggerKind) =>
		kind === 'restart_notice'
			? '预览下次重启'
			: kind === 'name_filter'
				? '试运行：历史玩家'
				: kind === 'ping_kick'
					? '检查试运行限制'
					: '试运行：最近 24 小时';
	const lines = (text: string) =>
		text
			.split(/[\n,]/)
			.map((w) => w.trim())
			.filter(Boolean);
	// A cleared number input binds null, not '': an optional count is sent only when it is a number.
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
					maxPlayers: typeof f.maxPlayers === 'number' ? f.maxPlayers : null
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
					maxBanAgeDays: Number(f.maxBanAgeDays),
					minAccountDays: Number(f.minAccountDays),
					privateProfiles: f.privateProfiles,
					bannedElsewhere: f.bannedElsewhere,
					watchlist: f.watchlist,
					kickAtScore: Number(f.kickAtScore) || null,
					spareReserved: f.spareReserved,
					reason: f.reason
				};
			case 'name_filter':
				return {
					characters: f.characters,
					extraScripts: f.characters === 'latin' ? f.extraScripts : [],
					allowSymbols: f.allowSymbols,
					minLetters: Number(f.minLetters),
					builtinWords: f.builtinWords,
					blocked: lines(f.blocked),
					allowed: lines(f.allowed),
					action: f.nameAction,
					spareReserved: f.spareReserved,
					reason: f.reason
				};
			case 'ping_kick':
				return {
					maxPingMs: Number(f.maxPingMs),
					durationSeconds: Number(f.durationSeconds),
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
			case 'kill_rate':
				return {
					windowMinutes: Number(f.windowMinutes),
					maxKills: Number(f.maxKills),
					headshotPct: Number(f.headshotPct),
					headshotMinKills: Number(f.headshotMinKills),
					cooldownMinutes: Number(f.cooldownMinutes)
				};
			case 'seed_reward':
				return {
					lowAt: Number(f.lowAt),
					untilFull: f.untilFull,
					fullAt: typeof f.fullAt === 'number' ? f.fullAt : null,
					minutes: Number(f.minutes),
					windowDays: Number(f.windowDays),
					slotDays: Number(f.slotDays),
					scope: f.slotScope,
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
			f.id ? '规则已保存。' : '规则已添加。'
		);
		if (ok) form = null;
	}
	const toggle = (t: TriggerView) =>
		run(
			() => api('PATCH', `${path}/${t.id}`, { enabled: !t.enabled }),
			t.enabled ? `“${t.name}”已停用。` : `“${t.name}”已启用。`
		);
	async function remove(t: TriggerView) {
		if (!(await confirmDialog(`确定删除规则“${t.name}”？`, { okLabel: '删除', danger: true })))
			return;
		if (await run(() => api('DELETE', `${path}/${t.id}`), '规则已删除。'))
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
				return `"${c.message}"${c.afterFaction ? ' · 选择阵营后' : ' · 入服时'}${c.onlyFirstVisit ? ' · 仅首次访问' : ''}`;
			case 'faction_change':
				return `"${c.message}"`;
			case 'broadcast':
				return `${(c.messages as string[]).length} 条消息，每 ${c.everyMinutes} 分钟轮播 · 在线 ${typeof c.maxPlayers === 'number' ? `${c.minPlayers}～${c.maxPlayers}` : `至少 ${c.minPlayers}`} 人`;
			case 'empty_reset':
				return `空服 ${c.afterMinutes} 分钟后切换至${c.map ? mapLabel(data.catalog, String(c.map)) : '所选地图'}`;
			case 'risk_kick': {
				const banAge = c.maxBanAgeDays ? `（最近 ${c.maxBanAgeDays} 天）` : '';
				const rules = [
					c.vacBans && `VAC 封禁${banAge}`,
					c.gameBans && `游戏封禁${banAge}`,
					c.minAccountDays &&
						`账号注册不足 ${c.minAccountDays} 天${c.privateProfiles ? '或资料私密' : ''}`,
					c.bannedElsewhere && '组织内其他服务器已封禁',
					c.watchlist && '关注名单',
					kickAtScoreOf(c) && `风险分 ≥${kickAtScoreOf(c)}`
				].filter(Boolean);
				return `${rules.join('、')}${c.spareReserved ? ' · 跳过预留席位玩家' : ''}`;
			}
			case 'name_filter': {
				const also = ((c.extraScripts as string[] | undefined) ?? []).map(
					(x) => SCRIPTS.find(([k]) => k === x)?.[1] ?? x
				);
				const blocked = ((c.blocked as string[] | undefined) ?? []).length;
				const allowed = ((c.allowed as string[] | undefined) ?? []).length;
				const lists = [c.builtinWords && '内置词库', blocked && `${blocked} 个禁用词`].filter(
					Boolean
				);
				return [
					c.characters === 'ascii'
						? '仅 ASCII 字符'
						: c.characters === 'latin'
							? `${['拉丁', ...also].join('、')}字母`
							: '',
					c.characters !== 'off' && c.allowSymbols ? '允许表情及符号' : '',
					c.minLetters ? `至少 ${c.minLetters} 个字母` : '',
					lists.length ? `${lists.join('及')}${allowed ? `，${allowed} 个例外` : ''}` : '',
					c.action === 'alert' ? '仅标记' : '踢出',
					c.spareReserved ? '跳过预留席位玩家' : ''
				]
					.filter(Boolean)
					.join(' · ');
			}
			case 'ping_kick':
				return `延迟超过 ${c.maxPingMs} 毫秒，持续 ${c.durationSeconds} 秒`;
			case 'restart_notice':
				return `"${c.message}"${c.leadMinutes ? ` · 提前 ${c.leadMinutes} 分钟提醒` : ''}${c.repeatMinutes ? ` · 每 ${c.repeatMinutes} 分钟重复` : ''} · 至少 ${c.minPlayers} 人在线`;
			case 'match_broadcast':
				return [
					c.endMessage ? `结束："${c.endMessage}"` : '',
					c.startMessage ? `开始："${c.startMessage}"` : ''
				]
					.filter(Boolean)
					.join(' · ')
					.concat(` · 至少 ${c.minPlayers} 人在线`);
			case 'team_kill':
				return [
					c.warnAt ? `第 ${c.warnAt} 次队友击杀时私信警告` : '',
					c.kickAt ? `第 ${c.kickAt} 次时踢出` : ''
				]
					.filter(Boolean)
					.join(' · ')
					.concat(' · 每次会话单独计算');
			case 'kill_rate':
				return [
					c.maxKills ? `${c.maxKills} 次击杀` : '',
					c.headshotPct ? `至少 ${c.headshotMinKills} 次击杀且爆头率达 ${c.headshotPct}%` : ''
				]
					.filter(Boolean)
					.join('或')
					.concat(`，观察窗口 ${c.windowMinutes} 分钟 · 仅标记 · 冷却 ${c.cooldownMinutes} 分钟`);
			case 'seed_reward':
				return `在线人数不超过 ${c.lowAt} 时留服 ${c.minutes} 分钟${c.untilFull === false ? '' : `，直到${typeof c.fullAt === 'number' ? `在线达到 ${c.fullAt} 人` : '服务器满员'}`}；统计近 ${c.windowDays} 天 · ${c.scope === 'server' ? '本服' : '组织所有服务器'}预留席位 ${c.slotDays} 天${c.message ? ' · 附带私信' : ''}`;
		}
	}
</script>

<div class="mb-5 space-y-4">
	<details class="mb-4 panel p-4">
		<summary class="cursor-pointer font-semibold"
			>强弱阵营平衡 · {data.skillBalance.rule.enabled ? '已启用' : '未启用'}（点击设置）</summary
		>
		<div class="mt-3">
			<SkillBalanceSettings data={data.skillBalance} serverId={data.server.id} />
		</div>
	</details>
	<details class="panel p-4">
		<summary class="cursor-pointer font-semibold"
			>禁止自行换边 · {data.factionLock.rule.enabled ? '已启用' : '未启用'}（点击设置）</summary
		>
		<div class="mt-4">
			<FactionLockSettings data={{ ...data.factionLock, teams: data.teams, server: data.server }} />
		</div>
	</details>
	<details class="panel p-4">
		<summary class="cursor-pointer font-semibold"
			>武器／载具限制 · {data.weaponRestriction.rule.enabled
				? '已启用'
				: '未启用'}（点击设置）</summary
		><WeaponRestrictionSettings
			serverId={data.server.id}
			data={data.weaponRestriction}
			feed={data.feed}
		/>
	</details>
</div>

<svelte:window
	onclick={() => (addOpen = false)}
	onkeydown={(e) => e.key === 'Escape' && (addOpen = false)}
/>

<p class="mb-4 text-[13px] text-mist-400">
	规则适用于{data.server.demo
		? '演示服务器'
		: '当前服务器'}。玩家加入、击杀或服务器进入低活跃状态时，规则会自动执行。每次操作都会显示在下方，并写入审计记录。
	<span class="chip">触发条件</span>。
</p>

<div class="mb-3 flex flex-wrap items-start gap-3">
	<div class="min-w-0 grow">
		<span class="label-sm mb-0">规则</span>
		<div class="mt-0.5 text-[12.5px] text-mist-400">
			{#if data.triggers.length}
				共 {data.triggers.length} 条规则 · {data.triggers.filter((t) => t.enabled).length} 条已启用
				{#if lastAction}· 最近操作 <span title={fmtTime(lastAction)}>{fmtAgo(lastAction, now)}</span
					>{/if}
				{#if failingCount}
					· <button
						type="button"
						class="cursor-pointer text-danger underline decoration-danger/50 underline-offset-2 hover:decoration-danger"
						aria-pressed={onlyFailing}
						onclick={() => (onlyFailing = !onlyFailing)}
						>{failingCount} 条失败{onlyFailing ? ' · 显示全部' : ''}</button
					>
				{/if}
			{:else}
				这台服务器尚无规则。
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
				添加规则 <span class="text-[10px] text-mist-600">▼</span>
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
						<div class="px-3 pt-2 pb-1 caps text-mist-600">{groupLabel[g]}</div>
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
		<!-- An off row fades its contents, not the panel: opacity on the panel would fade the ⋯ menu
		     too and trap it under the next row. -->
		<div class="panel py-3.5">
			<div class="flex items-start gap-3">
				<button
					type="button"
					role="switch"
					aria-checked={t.enabled}
					aria-label="{t.name}：{t.enabled ? '已启用' : '已停用'}"
					class="mt-1 h-[18px] w-8 shrink-0 cursor-pointer rounded-full border border-black transition disabled:cursor-not-allowed {t.enabled
						? 'bg-accent'
						: 'bg-ink-700 opacity-60'}"
					disabled={!admin || busy}
					onclick={() => toggle(t)}
				>
					<span
						class="block h-3 w-3 rounded-full bg-ink-950 transition-transform {t.enabled
							? 'translate-x-[15px]'
							: 'translate-x-[2px]'}"
					></span>
				</button>
				<div class="min-w-0 flex-1 {t.enabled ? '' : 'opacity-60'}">
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
						{#if h?.failing}<Badge tone="err">▲ 异常</Badge>{/if}
						<span class="chip">{label(t.kind)}</span>
					</div>
					<div class="mt-0.5 line-clamp-2 text-[13px] text-mist-400">
						{describe(t.kind, t.config)}
					</div>
					<!-- One of four shapes, most urgent first: failing, off, fired, never fired. -->
					<div class="mt-0.5 text-[12px] {h?.failing ? 'text-mist-100' : 'text-mist-600'}">
						{#if h?.failing}
							最近操作失败 · <span class="font-mono text-[11.5px] text-mist-400">{h.outcome}</span>
							· <span title={fmtTime(h.latest)}>{fmtAgo(h.latest, now)}</span>
							<button type="button" class="ml-1 btn btn-sm" onclick={() => seeActions(t)}
								>查看操作</button
							>
						{:else if !t.enabled}
							已停用 · {#if t.lastFiredAt}上次触发 <span title={fmtTime(t.lastFiredAt)}
									>{fmtAgo(t.lastFiredAt, now)}</span
								>{:else}从未触发{/if}
						{:else if t.lastFiredAt}
							上次触发 <span title={fmtTime(t.lastFiredAt)}>{fmtAgo(t.lastFiredAt, now)}</span>
							{#if h?.count}· {countLine(h)}{:else if t.fireCount}· 累计 {t.fireCount} 次操作{/if}
						{:else}
							从未触发{#if needs(t.kind)}
								· {needs(t.kind)}{/if}
						{/if}
					</div>
				</div>
				{#if admin}
					<RowMenu label="{t.name} 的操作">
						<button
							type="button"
							class="menu-item"
							role="menuitem"
							disabled={dryBusy}
							onclick={() => dryRun(t.kind, t.config, t.id, t.name)}>{dryLabel(t.kind)}</button
						>
						<button type="button" class="menu-item" role="menuitem" onclick={() => open(t.kind, t)}
							>编辑</button
						>
						<button
							type="button"
							class="menu-item"
							role="menuitem"
							onclick={() => open(t.kind, t, true)}>重复</button
						>
						<hr class="my-1 border-black" />
						<button
							type="button"
							class="menu-item text-danger!"
							role="menuitem"
							disabled={busy}
							onclick={() => remove(t)}>删除</button
						>
					</RowMenu>
				{/if}
			</div>
		</div>
	{:else}
		<div class="flex flex-col items-center gap-3 panel py-7 text-center">
			{#if admin && !onlyFailing}
				<p class="text-mist-100">多数服务器使用这两个默认项。</p>
				<div class="flex flex-wrap justify-center gap-2">
					<button type="button" class="btn btn-primary" onclick={() => open('welcome')}
						>＋欢迎私聊</button
					>
					<button type="button" class="btn" onclick={() => open('broadcast')}>＋定时广播</button>
				</div>
				<p class="max-w-[52ch] text-[12.5px] text-mist-600">
					也可以添加其他规则：{KINDS.filter((k) => k.kind !== 'welcome' && k.kind !== 'broadcast')
						.map((k) => k.label)
						.join(' · ')}.
				</p>
			{:else if onlyFailing}
				<p class="text-mist-600">没有异常规则。</p>
			{:else}
				<p class="text-mist-600">这台服务器尚无规则。</p>
			{/if}
		</div>
	{/each}
</div>

{#snippet placeholders(names: string[])}
	<div class="flex flex-wrap items-center gap-1 text-[12px] text-mist-600">
		<span class="mr-1">插入</span>
		{#each names as n (n)}
			<button
				type="button"
				class="chip cursor-pointer text-mist-100 transition hover:bg-white/12"
				title="在光标处插入 {'{' + n + '}'}"
				onclick={() => insert(n)}>{'{' + n + '}'}</button
			>
		{/each}
	</div>
{/snippet}

{#snippet dryResult(r: DryRunResult, title: string)}
	<div class="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px]">
		<span class="caps text-accent"
			>{r.kind === 'restart_notice' ? '下次重启' : '试运行'} · {title}</span
		>
		{#if r.kind === 'restart_notice'}
			<span><b class={r.fires ? 'text-warn' : 'text-ok'}>{r.fires}</b> 次广播</span>
		{:else if r.kind === 'name_filter'}
			<span
				>所有曾在本服游玩的玩家：符合条件者 <b class={r.fires ? 'text-warn' : 'text-ok'}
					>{r.fires}</b
				>
				名玩家</span
			>
		{:else}
			<span
				>模拟本服过去 24 小时：预计触发 <b class={r.fires ? 'text-warn' : 'text-ok'}>{r.fires}</b>
				次</span
			>
		{/if}
		<button
			type="button"
			class="ml-auto btn btn-sm btn-ghost"
			aria-label="关闭模拟预览"
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
					…以及 {r.fires - r.items.length} 更多
				</li>{/if}
		</ul>
	{/if}
	{#each r.notes as n (n)}<p class="note">{n}</p>{/each}
{/snippet}

{#if form}
	{@const f = form}
	<Modal
		title="{f.id ? '编辑' : '新建'} · {label(f.kind)}"
		wide={f.kind === 'empty_reset' || f.kind === 'name_filter'}
		onclose={() => (form = null)}
	>
		<form
			class="space-y-3"
			bind:this={formEl}
			onfocusin={(e) => {
				if (isText(e.target) && e.target.name !== 'name' && !e.target.dataset.plain)
					lastField = e.target;
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
						><span class="field-label">名称</span><input
							class="input"
							type="text"
							name="name"
							bind:value={f.name}
							maxlength="60"
							required
						/></label
					>
					<label class="flex items-end gap-2 pb-2.5 text-[13px]"
						><input type="checkbox" bind:checked={f.enabled} /> 已启用</label
					>
				</div>

				{#if f.kind === 'welcome'}
					<fieldset class="space-y-2">
						<legend class="field-label">私聊</legend>
						<input class="input" type="text" bind:value={f.message} maxlength="200" required />
						{@render placeholders(['name', 'faction', 'server', 'map', 'players', 'max'])}
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">时间</legend>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.afterFaction} /> 等待玩家选择阵营后再执行</label
						>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.onlyFirstVisit} /> 仅在玩家首次加入本服时执行</label
						>
					</fieldset>
					<p class="note">通过私聊发送，仅该玩家可见。</p>
				{:else if f.kind === 'faction_change'}
					<fieldset class="space-y-2">
						<legend class="field-label">私聊</legend>
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
					<p class="note">玩家从一个阵营转到另一个阵营时触发；初次加入后选阵营不会触发。</p>
				{:else if f.kind === 'broadcast'}
					<fieldset class="space-y-2">
						<legend class="field-label">每行一条消息，依次发送</legend>
						<textarea class="min-h-[100px] input" bind:value={f.messages} required></textarea>
						{@render placeholders(['server', 'map', 'players', 'max'])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">时间</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							每
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.everyMinutes}
								aria-label="每隔多少分钟"
								required
							/>
							分钟，至少
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.minPlayers}
								aria-label="最少玩家数"
							/>
							，最多
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.maxPlayers}
								aria-label="最多玩家数"
								placeholder="不限"
							/>
							人在线
						</div>
					</fieldset>
					<p class="note">
						留空表示不限制人数。暖服广播可以在达到目标人数后停止；每条广播最多 200 字符。
					</p>
				{:else if f.kind === 'empty_reset'}
					<fieldset class="space-y-2">
						<legend class="field-label">重置为</legend>
						<MapPicker bind:this={picker} serverId={id} catalog={data.catalog} />
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">时间</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							服务器连续空闲
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.afterMinutes}
								aria-label="空服持续时间（分钟）"
								required
							/>
							分钟后触发，每隔至少
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.cooldownMinutes}
								aria-label="重置冷却时间（分钟）"
							/>
							分钟
						</div>
					</fieldset>
					<p class="note">
						服务器空闲达到设定时间，且当前地图或模式与目标不同时触发。启用轮换时会把目标设为下一张地图并结束比赛；未启用轮换时直接请求切换地图。
					</p>
				{:else if f.kind === 'restart_notice'}
					<fieldset class="space-y-2">
						<legend class="field-label">窗口开启前提醒</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							发送
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1439"
								bind:value={f.leadMinutes}
								aria-label="提前提醒时间（分钟）"
							/>
							分钟前 <span class="text-mist-600">（设为 0 可关闭提前提醒）</span>
						</div>
						<input
							class="input"
							type="text"
							bind:value={f.leadMessage}
							maxlength="200"
							aria-label="提前提醒消息"
							disabled={!Number(f.leadMinutes)}
						/>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">窗口开启后</legend>
						<input
							class="input"
							type="text"
							bind:value={f.message}
							maxlength="200"
							aria-label="窗口开启后的消息"
							required
						/>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							每隔
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1440"
								bind:value={f.repeatMinutes}
								aria-label="每隔多少分钟重复"
							/>
							分钟，且本回合运行在 <span class="text-mist-600">（设为 0 则只发送一次）</span>
						</div>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">至少满足</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.minPlayers}
								aria-label="最少玩家数"
							/>
							人在线
						</div>
					</fieldset>
					{@render placeholders(['minutes', 'uptime', 'server', 'map', 'players', 'max'])}
					<p class="note">游戏服务器启动满 24 小时后，会在当时正在进行的回合结束时重启。</p>
				{:else if f.kind === 'match_broadcast'}
					<fieldset class="space-y-2">
						<legend class="field-label">比赛结束时</legend>
						<input
							class="input"
							type="text"
							bind:value={f.endMessage}
							maxlength="200"
							aria-label="比赛结束时的消息"
							placeholder="留空则不发送"
						/>
						{@render placeholders([
							'faction',
							'score',
							'scores',
							'cap',
							'previous',
							'mvp',
							'top',
							'map',
							'server',
							'players'
						])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">下一场开始时</legend>
						<input
							class="input"
							type="text"
							bind:value={f.startMessage}
							maxlength="200"
							aria-label="下场比赛开始时的消息"
							placeholder="留空则不发送"
						/>
						{@render placeholders(['map', 'previous', 'server', 'players', 'max'])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">至少满足</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.minPlayers}
								aria-label="最少玩家数"
							/>
							人在线
						</div>
					</fieldset>
					<p class="note">
						地图切换或阵营分数归零视为比赛结束；手动结束比赛或切换地图也计入。 {'{faction}'} 获胜方按比赛结束时的领先阵营判定；平分时会列出所有并列阵营。消息在该回合结束后的下一次轮询发送，繁忙服务器通常需要几秒，空闲服务器最多约半分钟。
					</p>
				{:else if f.kind === 'risk_kick'}
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">满足以下条件时踢出玩家</legend>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.bannedElsewhere} /> 曾在本组织其他服务器被封禁</label
						>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.watchlist} /> 在观察名单中</label
						>
						<div
							class="grid grid-cols-1 gap-x-4 gap-y-1.5 border-t border-black pt-2 sm:grid-cols-[1fr_auto] {data.steam
								? ''
								: 'text-mist-600'}"
						>
							<div class="space-y-1.5">
								<label class="flex items-center gap-2"
									><input type="checkbox" bind:checked={f.vacBans} disabled={!data.steam} /> 受到 VAC
									封禁</label
								>
								<label class="flex items-center gap-2"
									><input type="checkbox" bind:checked={f.gameBans} disabled={!data.steam} /> 游戏封禁</label
								>
								{#if f.vacBans || f.gameBans}
									<div
										class="flex flex-wrap items-center gap-2 pl-5 {data.steam
											? ''
											: 'text-mist-600'}"
									>
										只检查最近
										<input
											class="input w-24 text-right"
											type="number"
											min="0"
											max="36500"
											bind:value={f.maxBanAgeDays}
											disabled={!data.steam}
										/>
										天（0 表示不限时间）
									</div>
								{/if}
								<div class="flex flex-wrap items-center gap-2">
									，Steam 账号注册不足
									<input
										class="input w-20 text-right"
										type="number"
										min="0"
										max="3650"
										bind:value={f.minAccountDays}
										aria-label="Steam 账号年龄低于（天）"
										disabled={!data.steam}
									/>
									天 <span class="text-mist-600">（设为 0 可关闭）</span>
								</div>
								<label class="flex items-center gap-2 pl-5"
									><input
										type="checkbox"
										bind:checked={f.privateProfiles}
										disabled={!data.steam || !f.minAccountDays}
									/> 将资料未公开、账号年龄未知的玩家也视为账号过新</label
								>
							</div>
							<p
								class="max-w-[22ch] text-[12px] text-mist-600 sm:border-l sm:border-ink-700 sm:pl-3"
							>
								数据来自 Steam；玩家首次出现时获取，此后每天刷新。
							</p>
						</div>
						<label class="flex flex-wrap items-center gap-2 border-t border-black pt-2"
							>，建议风险分达到
							<input
								class="input w-[80px]"
								type="number"
								min="0"
								max="100"
								bind:value={f.kickAtScore}
							/>
							或以上，按玩家表显示的数据判断（设为 0 可关闭）</label
						>
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">从不踢出</legend>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.spareReserved} /> 持有预留位的玩家</label
						>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">踢出原因（玩家可见）</legend>
						<input class="input" type="text" bind:value={f.reason} maxlength="200" />
					</fieldset>
					<p class="note">踢出操作及命中的规则会记入审计日志。</p>
				{:else if f.kind === 'name_filter'}
					<div class="grid grid-cols-1 items-start gap-x-6 gap-y-3 sm:grid-cols-2">
						<div class="space-y-3">
							<fieldset class="space-y-2 text-[13px]">
								<legend class="field-label">名称允许使用的字符</legend>
								<select class="input" bind:value={f.characters} aria-label="字符规则">
									<option value="off">不限</option>
									<option value="latin">拉丁字母（允许 José、Müller 等）</option>
									<option value="ascii">仅限 ASCII 字符（美式键盘可输入）</option>
								</select>
								{#if f.characters === 'latin'}
									<div class="flex flex-wrap items-center gap-x-3.5 gap-y-1.5">
										<span class="text-mist-400">并且</span>
										{#each SCRIPTS as [value, name] (value)}
											<label class="flex items-center gap-1.5"
												><input type="checkbox" {value} bind:group={f.extraScripts} />
												{name}</label
											>
										{/each}
									</div>
								{/if}
								{#if f.characters !== 'off'}
									<label class="flex flex-wrap items-center gap-2 border-t border-black pt-2"
										><input type="checkbox" bind:checked={f.allowSymbols} /> 允许表情和符号
										<span class="text-mist-600">(★ 【 】 and the like)</span></label
									>
								{/if}
								<div class="flex flex-wrap items-center gap-2">
									至少
									<input
										class="input w-20 text-right"
										type="number"
										min="0"
										max="10"
										bind:value={f.minLetters}
										aria-label="名称最少字符数"
									/>
									个字母
									<span class="text-mist-600">（设为 0 可关闭；也会匹配 ____ 和 .... 等名称）</span>
								</div>
								{#if f.characters !== 'off'}
									<p class="text-[12px] text-mist-600">数字、空格和键盘标点始终允许。</p>
								{/if}
							</fieldset>
							<fieldset class="space-y-1.5 text-[13px]">
								<legend class="field-label">名称匹配时</legend>
								<label class="flex items-center gap-2"
									><input type="radio" value="kick" bind:group={f.nameAction} /> 踢出玩家</label
								>
								<label class="flex flex-wrap items-center gap-2"
									><input type="radio" value="alert" bind:group={f.nameAction} /> 仅标记
									<span class="text-mist-600">（仅记录审计日志并通知 Discord，不踢出玩家）</span
									></label
								>
								<label class="flex items-center gap-2 border-t border-black pt-2"
									><input type="checkbox" bind:checked={f.spareReserved} /> 始终跳过持有预留位的玩家</label
								>
							</fieldset>
						</div>
						<div class="space-y-3 sm:border-l sm:border-black sm:pl-6">
							<fieldset class="space-y-2 text-[13px]">
								<legend class="field-label">名称禁止包含的词</legend>
								<label class="flex flex-wrap items-center gap-2"
									><input type="checkbox" bind:checked={f.builtinWords} /> 内置英文违禁词表
									<span class="text-mist-600">（包含歧视和仇恨词；其他不当用语需自行添加）</span
									></label
								>
								<textarea
									class="input font-mono text-[12.5px]"
									rows="7"
									data-plain
									bind:value={f.blocked}
									aria-label="禁止的词，每行一个"
									placeholder="每行一个词"></textarea>
								<p class="text-[12px] text-mist-600">
									可识别大小写变化、数字替代（如 n4z1）、相似字母、重复字母和逐字拼写（如
									n.a.z.i）。每行一个词，仅限字母、数字和空格，最多 200 项。
								</p>
							</fieldset>
							<fieldset class="space-y-2 text-[13px]">
								<legend class="field-label">排除</legend>
								<textarea
									class="input font-mono text-[12.5px]"
									rows="3"
									data-plain
									bind:value={f.allowed}
									aria-label="允许的词，每行一个"></textarea>
								<p class="text-[12px] text-mist-600">不会受到此规则影响的名称或名称片段。</p>
							</fieldset>
						</div>
					</div>
					{#if f.nameAction === 'kick'}
						<fieldset class="space-y-2">
							<legend class="field-label">踢出原因（玩家可见）</legend>
							<input class="input" type="text" bind:value={f.reason} maxlength="200" />
							{@render placeholders(['why', 'name', 'server'])}
							<p class="text-[12px] text-mist-600">
								{'{why}'} 向玩家说明违规类型（例如“名称包含拉丁字母以外的字符”），不会重复显示违规词。
							</p>
						</fieldset>
					{/if}
					<p class="note">
						玩家加入时检查名称；游戏中途改名的玩家会在下次加入时检查。启用违禁词规则前请先运行模拟预览。
					</p>
				{:else if f.kind === 'ping_kick'}
					<fieldset class="space-y-2">
						<legend class="field-label">延迟持续高于此值时踢出</legend>
						<div class="flex flex-wrap items-center gap-2 text-[13px]">
							<input
								class="input w-24 text-right"
								type="number"
								min="1"
								max="2000"
								bind:value={f.maxPingMs}
								aria-label="最高延迟（毫秒）"
								required
							/>
							毫秒，持续至少
							<input
								class="input w-24 text-right"
								type="number"
								min="1"
								max="3600"
								bind:value={f.durationSeconds}
								aria-label="高延迟持续时间（秒）"
								required
							/>
							秒
						</div>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">踢出原因（玩家可见）</legend>
						<input class="input" type="text" bind:value={f.reason} maxlength="200" />
					</fieldset>
					<p class="note">
						第一次采样到高延迟时开始计时。延迟降到阈值以下、无法获取延迟、玩家离开或玩家列表未按时采样时会重新计时。
					</p>
				{:else if f.kind === 'team_kill'}
					<fieldset class="space-y-2">
						<legend class="field-label">私聊</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							从
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="100"
								bind:value={f.warnAt}
								aria-label="误杀队友达到多少时私聊提醒"
							/>
							次误杀队友；达到阈值后的每次误杀都会触发
							<span class="text-mist-600">（设为 0 可关闭）</span>
						</div>
						<input
							class="input"
							type="text"
							bind:value={f.warnMessage}
							maxlength="200"
							aria-label="私聊"
							disabled={!Number(f.warnAt)}
						/>
						{@render placeholders(['name', 'victim', 'count', 'server', 'map'])}
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">踢出</legend>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[13px]">
							At
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="100"
								bind:value={f.kickAt}
								aria-label="误杀队友达到多少时踢出"
							/>
							误杀队友 <span class="text-mist-600">（设为 0 可关闭）</span>
						</div>
						<input
							class="input"
							type="text"
							bind:value={f.kickReason}
							maxlength="200"
							aria-label="踢出原因"
							disabled={!Number(f.kickAt)}
						/>
					</fieldset>
					<p class="note">误杀数据来自游戏击杀事件，按玩家当前场次分别统计。</p>
				{:else if f.kind === 'kill_rate'}
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">在以下条件标记玩家</legend>
						<div class="flex flex-wrap items-center gap-2">
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="1000"
								bind:value={f.maxKills}
								aria-label="窗口内达到多少击杀时标记"
							/>
							次击杀 <span class="text-mist-600">（设为 0 可关闭）</span>
						</div>
						<div class="flex flex-wrap items-center gap-2">
							or
							<input
								class="input w-20 text-right"
								type="number"
								min="0"
								max="100"
								bind:value={f.headshotPct}
								aria-label="爆头率达到多少时标记"
							/>
							% 爆头率，基于
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1000"
								bind:value={f.headshotMinKills}
								aria-label="计算爆头率的最低击杀数"
								disabled={!Number(f.headshotPct)}
							/>
							次击杀 <span class="text-mist-600">（设为 0% 可关闭）</span>
						</div>
						<div class="flex flex-wrap items-center gap-2">
							在
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="60"
								bind:value={f.windowMinutes}
								aria-label="时间范围（分钟）"
								required
							/>
							分钟
						</div>
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">再次标记同一玩家需间隔</legend>
						<div class="flex flex-wrap items-center gap-2">
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1440"
								bind:value={f.cooldownMinutes}
								aria-label="再次标记的间隔（分钟）"
								required
							/>
							分钟
						</div>
					</fieldset>
					<p class="note">
						统计击杀事件中的手持武器击杀。命中规则只会记录到审计日志并通知 Discord，不会踢出玩家。
					</p>
				{:else if f.kind === 'seed_reward'}
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">计入暖服</legend>
						<div class="flex flex-wrap items-center gap-2">
							在线人数不超过
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1000"
								bind:value={f.lowAt}
								aria-label="计入暖服的最大在线人数"
								required
							/>
							人在线
						</div>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={f.untilFull} /> 仅在服务器达到目标人数且该玩家仍在线时生效</label
						>
						<div
							class="flex flex-wrap items-center gap-2 pl-5 {f.untilFull ? '' : 'text-mist-600'}"
						>
							满员人数至少
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="1000"
								bind:value={f.fullAt}
								aria-label="达到多少玩家视为满员"
								placeholder="limit"
								disabled={!f.untilFull}
							/>
							名玩家 <span class="text-mist-600">（留空则使用服务器自身上限）</span>
						</div>
					</fieldset>
					<fieldset class="space-y-1.5 text-[13px]">
						<legend class="field-label">奖励</legend>
						<div class="flex flex-wrap items-center gap-2">
							<input
								class="input w-24 text-right"
								type="number"
								min="1"
								max="129600"
								bind:value={f.minutes}
								aria-label="所需暖服时间（分钟）"
								required
							/>
							在最近
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="90"
								bind:value={f.windowDays}
								aria-label="统计过去的天数"
								required
							/>
							天内达到暖服时长后，获得有效期为
							<input
								class="input w-20 text-right"
								type="number"
								min="1"
								max="365"
								bind:value={f.slotDays}
								aria-label="预留位有效期（天）"
								required
							/>
							天
						</div>
						<div class="flex flex-wrap gap-x-4 gap-y-1">
							<label class="flex items-center gap-2 {canSlotHere ? '' : 'text-mist-600'}"
								><input
									type="radio"
									bind:group={f.slotScope}
									value="server"
									disabled={!canSlotHere}
								/> 仅在本服</label
							>
							<label class="flex items-center gap-2 {canSlotOrg ? '' : 'text-mist-600'}"
								><input type="radio" bind:group={f.slotScope} value="org" disabled={!canSlotOrg} />
								在组织的所有服务器上生效</label
							>
						</div>
					</fieldset>
					<fieldset class="space-y-2">
						<legend class="field-label">授予时发送私聊，留空则不发送</legend>
						<input class="input" type="text" bind:value={f.message} maxlength="200" />
						{@render placeholders(['name', 'server', 'minutes', 'until', 'days', 'players', 'max'])}
					</fieldset>
					<p class="note">
						勾选后，暖服时间会暂存，直到服务器达到目标人数且玩家仍在线；提前离开则作废。未勾选时，每分钟低人数在线时间都会立即计入。本服预留位写入本服名单；全组织预留位写入组织名单，本服立即应用，其他服务器下次同步时应用。预留位到期后可重新获得；已有本服预留位的玩家会跳过。
					</p>
				{/if}

				<div class="rounded-ctl border border-black bg-ink-950 px-3 py-2 text-[13px]">
					<span class="mr-2 caps text-accent">读取结果</span>
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
					>{dryBusy ? '处理中…' : dry && dryFor === 'form' ? '返回表单' : dryLabel(f.kind)}</button
				>
				<button type="button" class="btn" data-close onclick={() => (form = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}
					>{f.id ? '保存' : '添加规则'}</button
				>
			</div>
		</form>
	</Modal>
{/if}

{#if data.triggers.length || deliveries.length}
	<div class="mt-4 scroll-mt-4 panel" bind:this={actionsPanel}>
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<span class="label-sm mb-0">近期操作</span>
			<span class="text-[12.5px] text-mist-600">规则执行记录，最新优先</span>
			<div class="flex w-full flex-wrap gap-2 sm:ml-auto sm:w-auto">
				<select
					class="input w-auto pr-[30px] {ruleFilter ? 'border-accent' : ''}"
					aria-label="仅此规则"
					bind:value={ruleFilter}
				>
					<option value="">全部规则</option>
					{#each ruleNames as name (name)}<option value={name}>{name}</option>{/each}
				</select>
				<select
					class="input w-auto pr-[30px] {stateFilter ? 'border-accent' : ''}"
					aria-label="仅此状态"
					bind:value={stateFilter}
				>
					<option value="">任意状态</option>
					{#each STATES as s (s)}<option value={s}>{s}</option>{/each}
				</select>
				<input
					class="input w-full sm:w-52"
					type="search"
					placeholder="按操作、目标或结果筛选…"
					aria-label="筛选近期操作"
					bind:value={deliverySearch}
				/>
			</div>
		</div>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<SortHeader sort={deliverySort} key="when">时间</SortHeader>
						<SortHeader sort={deliverySort} key="rule">规则</SortHeader>
						<SortHeader sort={deliverySort} key="action">操作</SortHeader>
						<SortHeader sort={deliverySort} key="target">目标</SortHeader>
						<SortHeader sort={deliverySort} key="state">状态</SortHeader>
						<SortHeader sort={deliverySort} key="result">结果</SortHeader>
					</tr>
				</thead>
				<tbody>
					{#each deliveryRows as d (d.id)}
						<tr>
							<td class="whitespace-nowrap">{fmtTime(d.createdAt)}</td>
							<td>{d.triggerName}</td>
							<td class="font-mono text-[12px]">{actionLabel(d.action)}</td>
							<td class="font-mono text-[12px]"
								>{#if isSteamId(d.target)}<a class="link" href="/server/{id}/players/{d.target}"
										>{d.target}</a
									>{:else}{d.target}{/if}</td
							>
							<td
								><Badge
									tone={stateTone(d.state)}
									title={d.state === 'unknown'
										? '已发送，但游戏服务器未响应；不会自动重试。'
										: undefined}>{d.state}</Badge
								></td
							>
							<td class="text-mist-400">{d.outcome}</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-6 text-center text-mist-600"
								>{deliveries.length ? '没有符合条件的记录。' : '暂无操作记录。'}</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
{/if}
