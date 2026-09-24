// The permission vocabulary. A capability is one thing a person (or an API key) may do on a
// server; an organisation's roles are named sets of them, and org owners and the site owner hold
// every one. This module is pure and client-safe: pages, the role editor and the server all read
// the same list, so the browser can hide a button for exactly the reason the API refuses it.
import type { ListKind } from './types';

export const CAPABILITIES = [
	'server.view',
	'chat.send',
	'players.moderate',
	'match.control',
	'rotation.edit',
	'players.notes',
	'rotation.save',
	'players.notes.manage',
	'integrity.view',
	'bans.manage',
	'slots.manage',
	'lists.ban',
	'lists.reserve',
	'config.apply',
	'automation.manage',
	'audit.read',
	'rcon.raw'
] as const;

export type Capability = (typeof CAPABILITIES)[number];

/** Every role must be able to see the server it is a role on; nothing else is mandatory. */
export const VIEW: Capability = 'server.view';

export type CapabilityGroup = 'read' | 'play' | 'moderate' | 'manage';

export const CAPABILITY_GROUPS: { key: CapabilityGroup; label: string; hint: string }[] = [
	{ key: 'read', label: '查看', hint: '查看服务器状态，不执行操作。' },
	{ key: 'play', label: '比赛管理', hint: '管理日常比赛。' },
	{ key: 'moderate', label: '玩家管理', hint: '管理封禁、预留位和组织名单。' },
	{ key: 'manage', label: '系统管理', hint: '管理设置、自动化和原始接口。' }
];

export interface CapabilityInfo {
	label: string;
	/** what it unlocks, in the words the panel uses */
	hint: string;
	group: CapabilityGroup;
}

export const CAPABILITY_INFO: Record<Capability, CapabilityInfo> = {
	'server.view': {
		label: '查看服务器',
		hint: '查看状态、玩家、击杀、地图轮换、封禁、预留位、分析、排行榜和玩家统计。',
		group: 'read'
	},
	'chat.send': {
		label: '聊天与广播',
		hint: '向全服广播，或向单个玩家发送私聊。',
		group: 'play'
	},
	'players.moderate': {
		label: '踢出、击杀与换队',
		hint: '踢出或击杀玩家，或将其移到另一阵营。',
		group: 'play'
	},
	'match.control': {
		label: '比赛控制',
		hint: '结束或重开比赛、切换地图、设置下一张地图或天气。',
		group: 'play'
	},
	'rotation.edit': {
		label: '实时地图轮换',
		hint: '在运行中的服务器上添加、移除或重新排列轮换地图。',
		group: 'play'
	},
	'players.notes': {
		label: '备注与观察名单',
		hint: '查看或添加玩家备注、删除自己的备注、管理观察名单并查看原因。',
		group: 'play'
	},
	'bans.manage': { label: '封禁', hint: '在本服封禁或解除封禁。', group: 'moderate' },
	'slots.manage': {
		label: '预留位',
		hint: '在本服分配或撤回预留位，并查看每项备注；旧版服务器通过配置文件同步。',
		group: 'moderate'
	},
	'lists.ban': {
		label: '组织封禁名单',
		hint: '通过组织名单在所有服务器封禁或解封，并同步到服务器；玩家档案中可查看记录。',
		group: 'moderate'
	},
	'lists.reserve': {
		label: '组织预留位',
		hint: '通过组织名单在所有服务器分配或撤回预留位，并同步到服务器；玩家档案中可查看记录。',
		group: 'moderate'
	},
	'players.notes.manage': {
		label: '管理他人备注',
		hint: '删除任何人撰写的玩家备注。',
		group: 'moderate'
	},
	'integrity.view': {
		label: '风控案件',
		hint: '查看本服的社区风控评分、证据案件和复核状态。',
		group: 'moderate'
	},
	'rotation.save': {
		label: '保存地图轮换',
		hint: '保存地图轮换并启用或关闭轮换模式。',
		group: 'manage'
	},
	'config.apply': {
		label: '配置与设置',
		hint: '读取、验证和应用配置文件，调整得分周期与图片并测试连接。',
		group: 'manage'
	},
	'automation.manage': {
		label: '自动化',
		hint: '查看规则及执行结果，创建、编辑、模拟或删除规则。',
		group: 'manage'
	},
	'audit.read': {
		label: '审计记录',
		hint: '查看本服所有管理员操作及游戏服务器原始 RCON 日志。',
		group: 'manage'
	},
	'rcon.raw': {
		label: '原始 RCON',
		hint: '直接调用游戏服务器的 /v1 接口（配置文件除外）。',
		group: 'manage'
	}
};

/**
 * Each of the organisation's lists has a capability of its own: holding it on any server of the
 * org opens that list, which then applies on every server of it.
 */
export const LIST_CAPABILITY: Record<ListKind, Capability> = {
	ban: 'lists.ban',
	reserve: 'lists.reserve'
};

/** The org lists a set of capabilities may edit, ban list first. */
export const listKindsIn = (caps: Iterable<string>): ListKind[] => {
	const held = new Set(caps);
	return (['ban', 'reserve'] as const).filter((k) => held.has(LIST_CAPABILITY[k]));
};

/** Capabilities in display order, grouped for the role editor. */
export const capabilitiesByGroup = (): {
	group: (typeof CAPABILITY_GROUPS)[number];
	caps: Capability[];
}[] =>
	CAPABILITY_GROUPS.map((group) => ({
		group,
		caps: CAPABILITIES.filter((c) => CAPABILITY_INFO[c].group === group.key)
	}));

// ---- built-in roles ----------------------------------------------------------------------------

export const BUILTIN_ROLES = ['viewer', 'operator', 'admin'] as const;
export type BuiltinRole = (typeof BUILTIN_ROLES)[number];

const OPERATOR: Capability[] = [
	'server.view',
	'chat.send',
	'players.moderate',
	'match.control',
	'rotation.edit',
	'players.notes'
];

/** What each built-in role starts with; owners may change them per organisation. */
export const BUILTIN_CAPABILITIES: Record<BuiltinRole, Capability[]> = {
	viewer: ['server.view'],
	operator: OPERATOR,
	admin: [...CAPABILITIES]
};

export const isBuiltinRole = (v: unknown): v is BuiltinRole =>
	typeof v === 'string' && (BUILTIN_ROLES as readonly string[]).includes(v);

// ---- helpers ------------------------------------------------------------------------------------

export const isCapability = (v: unknown): v is Capability =>
	typeof v === 'string' && (CAPABILITIES as readonly string[]).includes(v);

/**
 * Turns a request body or a stored jsonb value into a clean capability list: unknown strings are
 * an error (a typo must not silently grant nothing), duplicates collapse, order is canonical.
 */
export function parseCapabilities(raw: unknown): Capability[] {
	if (!Array.isArray(raw)) throw new Error('capabilities must be a list.');
	const seen = new Set<Capability>();
	for (const v of raw) {
		if (!isCapability(v)) throw new Error(`Unknown capability: ${String(v).slice(0, 40)}`);
		seen.add(v);
	}
	return CAPABILITIES.filter((c) => seen.has(c));
}

/** Stored values are trusted but may predate a rename; keep only what is still known. */
export const knownCapabilities = (raw: unknown): Capability[] =>
	Array.isArray(raw) ? CAPABILITIES.filter((c) => raw.includes(c)) : [];

export const can = (caps: Iterable<string> | null | undefined, cap: Capability): boolean => {
	if (!caps) return false;
	if (caps instanceof Set) return caps.has(cap);
	for (const c of caps) if (c === cap) return true;
	return false;
};

/** "View · Chat · Bans" for tables and tooltips. */
export const capabilitySummary = (caps: Iterable<string> | null | undefined): string =>
	knownCapabilities([...(caps ?? [])])
		.map((c) => CAPABILITY_INFO[c].label)
		.join(' · ');
