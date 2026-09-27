import { z } from 'zod';

export const metricLabels = {
	online: '在线人数',
	kills: '在线玩家总击杀',
	deaths: '在线玩家总死亡',
	averagePing: '平均延迟（ms）',
	cash: '在线玩家总现金'
} as const;
export const columnLabels = {
	name: '玩家',
	steamId: 'SteamID',
	faction: '阵营',
	kills: '击杀',
	deaths: '死亡',
	cash: '现金',
	ping: '延迟'
} as const;
const id = z
	.string()
	.regex(/^[a-z][a-z0-9-]{1,47}$/, '标识须为 2～48 位小写字母、数字或连字符，以字母开头');
const widget = z.discriminatedUnion('type', [
	z
		.object({
			type: z.literal('metric'),
			title: z.string().min(1).max(80),
			metric: z.enum(['online', 'kills', 'deaths', 'averagePing', 'cash'])
		})
		.strict(),
	z
		.object({
			type: z.literal('text'),
			title: z.string().min(1).max(80),
			text: z.string().max(2000)
		})
		.strict(),
	z
		.object({
			type: z.literal('players'),
			title: z.string().min(1).max(80),
			columns: z
				.array(z.enum(['name', 'steamId', 'faction', 'kills', 'deaths', 'cash', 'ping']))
				.min(1)
				.max(7),
			limit: z.number().int().min(1).max(50).default(10),
			sortBy: z.enum(['kills', 'deaths', 'cash', 'ping']).default('kills')
		})
		.strict()
]);
export const pluginManifestSchema = z
	.object({
		apiVersion: z.literal(1),
		id,
		name: z.string().trim().min(1).max(80),
		description: z.string().max(500).default(''),
		version: z
			.string()
			.regex(/^\d+\.\d+\.\d+$/)
			.default('1.0.0'),
		renderer: id.default('cards'),
		style: z
			.object({
				accent: z
					.string()
					.regex(/^#[0-9a-fA-F]{6}$/)
					.default('#69d6e3'),
				columns: z.number().int().min(1).max(3).default(2),
				density: z.enum(['comfortable', 'compact']).default('comfortable')
			})
			.strict()
			.default({ accent: '#69d6e3', columns: 2, density: 'comfortable' }),
		widgets: z.array(widget).max(12).default([])
	})
	.strict()
	.superRefine((manifest, ctx) => {
		if (manifest.renderer === 'cards' && !manifest.widgets.length)
			ctx.addIssue({ code: 'custom', path: ['widgets'], message: '卡片插件至少需要一个组件' });
	});
export type PluginManifest = z.infer<typeof pluginManifestSchema>;
export interface PluginSnapshot {
	apiVersion: 1;
	server: { id: string; name: string; map: string | null };
	statusAt: string | null;
	playersAt: string | null;
	stale: boolean;
	metrics: Record<keyof typeof metricLabels, number | null>;
	players: {
		name: string;
		steamId: string;
		faction: string | null;
		kills: number | null;
		deaths: number | null;
		cash: number | null;
		ping: number | null;
	}[];
	playersTruncated: boolean;
}
export interface PluginComponentProps {
	plugin: PluginManifest;
	snapshot: PluginSnapshot | null;
	loading: boolean;
	error: string | null;
}
export interface PersonalPlugin {
	manifest: PluginManifest;
	serverId: string | null;
	enabled: boolean;
	updatedAt: string;
}
