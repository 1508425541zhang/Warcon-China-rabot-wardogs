// Runtime settings the site owner may change without a deploy: observation cadences, budgets,
// sign-in rules. Stored in site_settings (key + JSON), bounds enforced here, audited by the caller,
// re-read by the worker every few seconds. Env vars only seed the defaults on a fresh install.
import { eq } from 'drizzle-orm';
import type { Env } from './env';
import { ApiError } from './http';
import { siteSettings } from './db/schema';

export interface SettingSpec {
	label: string;
	help: string;
	/** 'choice' renders a select over `options` (the value is still a number) */
	unit: 'ms' | 'count' | 'days' | 'choice';
	options?: readonly { value: number; label: string }[];
	default: number;
	min: number;
	max: number;
	group: 'observation' | 'delivery' | 'housekeeping' | 'accounts';
}

/** Values of the `authEnforce` setting: whom the sign-in rules are enforced on. */
export const AUTH_ENFORCE = { advise: 0, privileged: 1, everyone: 2 } as const;

export const SETTINGS = {
	watchedPlayersMs: {
		label: '有人查看：玩家',
		help: '有人打开服务器页面时读取玩家列表的间隔。',
		unit: 'ms',
		default: 1000,
		min: 500,
		max: 60_000,
		group: 'observation'
	},
	watchedStatusMs: {
		label: '有人查看：状态',
		help: '有人打开服务器页面时读取比分、地图和计时器的间隔。',
		unit: 'ms',
		default: 2000,
		min: 1000,
		max: 60_000,
		group: 'observation'
	},
	hotPlayersMs: {
		label: '有玩家在线：玩家',
		help: '服务器有玩家但无人查看时读取玩家列表的间隔；决定多快发现新加入的玩家。',
		unit: 'ms',
		default: 2000,
		min: 1000,
		max: 120_000,
		group: 'observation'
	},
	hotStatusMs: {
		label: '有玩家在线：状态',
		help: '服务器有玩家但无人查看时读取状态的间隔。',
		unit: 'ms',
		default: 5000,
		min: 1000,
		max: 120_000,
		group: 'observation'
	},
	idleMs: {
		label: '空服务器',
		help: '服务器无人在线时的检查间隔；首位玩家加入后会在此时间内被发现。',
		unit: 'ms',
		default: 30_000,
		min: 5000,
		max: 600_000,
		group: 'observation'
	},
	offlineMs: {
		label: '无法连接：首次重试',
		help: '服务器停止响应后的首次重试间隔；之后每次失败都会加倍。',
		unit: 'ms',
		default: 30_000,
		min: 5000,
		max: 600_000,
		group: 'observation'
	},
	offlineMaxMs: {
		label: '无法连接：最长等待',
		help: '重试间隔不会超过此值。',
		unit: 'ms',
		default: 120_000,
		min: 5000,
		max: 3_600_000,
		group: 'observation'
	},
	watchLeaseMs: {
		label: '查看状态保留时间',
		help: '最后一个浏览器关闭页面后，服务器继续使用高频检查的时间。',
		unit: 'ms',
		default: 15_000,
		min: 5000,
		max: 120_000,
		group: 'observation'
	},
	concurrency: {
		label: '并发检查数量',
		help: '工作进程可同时查询的服务器数量。这是进程保护上限，不是检查间隔；建议低于 256。',
		unit: 'count',
		default: 128,
		min: 1,
		max: 1024,
		group: 'observation'
	},
	sampleMs: {
		label: '统计数据心跳',
		help: '服务器运行时状态变化会写入采样记录；即使无变化，也至少按此间隔写入一次。',
		unit: 'ms',
		default: 20_000,
		min: 5000,
		max: 300_000,
		group: 'housekeeping'
	},
	sessionHeartbeatMs: {
		label: '玩家会话心跳',
		help: '更新在线会话最近观测时间和统计数据的间隔。',
		unit: 'ms',
		default: 30_000,
		min: 5000,
		max: 300_000,
		group: 'housekeeping'
	},
	listsSnapshotMs: {
		label: '封禁列表快照',
		help: '重新读取各服务器封禁列表和预留席位的间隔。',
		unit: 'ms',
		default: 300_000,
		min: 30_000,
		max: 3_600_000,
		group: 'housekeeping'
	},
	listSyncMs: {
		label: '组织列表同步',
		help: '将组织封禁和预留席位列表重新应用到各服务器的间隔。',
		unit: 'ms',
		default: 60_000,
		min: 10_000,
		max: 3_600_000,
		group: 'housekeeping'
	},
	outboxLeaseMs: {
		label: '动作发送租约',
		help: '已领取的自动化动作在多长时间后可由下一轮处理重新领取。',
		unit: 'ms',
		default: 45_000,
		min: 5000,
		max: 300_000,
		group: 'delivery'
	},
	outboxMaxAgeMs: {
		label: '过期动作截止时间',
		help: '超过此时长的动作将被跳过，不再发送（迟到两分钟的欢迎消息已无意义）。',
		unit: 'ms',
		default: 120_000,
		min: 10_000,
		max: 3_600_000,
		group: 'delivery'
	},
	authEnforce: {
		label: '登录安全规则',
		help: '每个账号应设置两种独立登录方式；使用密码时还需第二因素。“仅提醒”只显示提示。“要求管理员遵守”会在宽限期结束后限制平台和组织所有者，以及有封禁、配置、自动化或原始 RCON 权限的成员。“要求所有人遵守”则适用于所有账号。',
		unit: 'choice',
		options: [
			{ value: 0, label: '仅提醒' },
			{ value: 1, label: '要求管理员遵守' },
			{ value: 2, label: '要求所有人遵守' }
		],
		default: 0,
		min: 0,
		max: 2,
		group: 'accounts'
	},
	authGraceDays: {
		label: '所有者登录宽限期',
		help: '新登录安全规则生效后，所有者从首次登录起有多少天可以继续使用面板；期满后必须满足规则。',
		unit: 'days',
		default: 14,
		min: 0,
		max: 365,
		group: 'accounts'
	},
	authMemberGraceDays: {
		label: '成员登录宽限期',
		help: '成员适用的宽限期。组织所有者始终可以重置成员的登录方式。',
		unit: 'days',
		default: 30,
		min: 0,
		max: 365,
		group: 'accounts'
	}
} as const satisfies Record<string, SettingSpec>;

export type SettingKey = keyof typeof SETTINGS;
export type Settings = { [K in SettingKey]: number };
export const SETTING_KEYS = Object.keys(SETTINGS) as SettingKey[];

const clamp = (key: SettingKey, v: number) =>
	Math.min(SETTINGS[key].max, Math.max(SETTINGS[key].min, Math.round(v)));

function defaults(env?: Pick<Env, 'POLL_SECONDS' | 'POLL_CONCURRENCY'>): Settings {
	const out = {} as Settings;
	for (const k of SETTING_KEYS) out[k] = SETTINGS[k].default;
	// Legacy env seeds, honoured only until the owner saves a value.
	const poll = Number(env?.POLL_SECONDS);
	if (Number.isFinite(poll) && poll > 0) out.sampleMs = clamp('sampleMs', poll * 1000);
	const conc = Number(env?.POLL_CONCURRENCY);
	if (Number.isInteger(conc) && conc > 0) out.concurrency = clamp('concurrency', conc);
	return out;
}

let current: Settings = defaults();
let version = 0;

/** The effective settings right now (synchronous; loadSettings keeps it fresh). */
export const settings = (): Settings => current;
/** Bumps whenever the effective values change; the worker watches it to re-plan cadences. */
export const settingsVersion = (): number => version;

/** Reads the table over the defaults. Called at startup and by the worker every few seconds. */
export async function loadSettings(env: Env): Promise<Settings> {
	const rows = await env.db.select().from(siteSettings);
	const next = defaults(env);
	for (const r of rows) {
		const key = r.key as SettingKey;
		if (!(key in SETTINGS)) continue;
		const raw = r.value as { n?: unknown } | number | string | null;
		const v = Number(raw && typeof raw === 'object' ? raw.n : raw);
		if (Number.isFinite(v)) next[key] = clamp(key, v);
	}
	if (SETTING_KEYS.some((k) => next[k] !== current[k])) version++;
	current = next;
	return current;
}

/** Validates and stores a partial update; returns the keys that changed. Throws 400 on bad input. */
export async function saveSettings(
	env: Env,
	patch: Record<string, unknown>,
	updatedBy: string | null
): Promise<{ settings: Settings; changed: Partial<Settings> }> {
	await loadSettings(env); // compare against what is stored, not this process's cache
	const problems: string[] = [];
	const updates: Partial<Settings> = {};
	for (const [k, raw] of Object.entries(patch)) {
		if (!(k in SETTINGS)) {
			problems.push(`Unknown setting '${k}'.`);
			continue;
		}
		const key = k as SettingKey;
		const v = Number(raw);
		const spec = SETTINGS[key];
		if (!Number.isFinite(v) || v < spec.min || v > spec.max) {
			problems.push(`${spec.label} must be between ${spec.min} and ${spec.max} ${spec.unit}.`);
			continue;
		}
		const value = Math.round(v);
		if (value !== current[key]) updates[key] = value;
	}
	if (problems.length) throw new ApiError(400, problems.join(' '), 'bad_setting');
	const now = new Date();
	for (const [k, n] of Object.entries(updates)) {
		const value = { n };
		await env.db
			.insert(siteSettings)
			.values({ key: k, value, updatedAt: now, updatedBy })
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value, updatedAt: now, updatedBy }
			});
	}
	if (Object.keys(updates).length) {
		current = { ...current, ...updates };
		version++;
	}
	return { settings: current, changed: updates };
}

/** Resets a key to its default (removes the stored row). */
export async function resetSetting(env: Env, key: string): Promise<void> {
	if (!(key in SETTINGS)) throw new ApiError(400, `Unknown setting '${key}'.`, 'bad_setting');
	await env.db.delete(siteSettings).where(eq(siteSettings.key, key));
	await loadSettings(env);
}

export interface SettingView extends SettingSpec {
	key: SettingKey;
	value: number;
	stored: boolean;
}

/** Every setting with its effective value, for the owner's settings page. */
export async function settingsView(env: Env): Promise<SettingView[]> {
	await loadSettings(env);
	const stored = new Set(
		(await env.db.select({ key: siteSettings.key }).from(siteSettings)).map((r) => r.key)
	);
	return SETTING_KEYS.map((key) => ({
		key,
		...SETTINGS[key],
		value: current[key],
		stored: stored.has(key)
	}));
}
