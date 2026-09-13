// How a Discord status card looks; picked per webhook, built by webhook-status-core.ts.
export const STATUS_STYLES = ['banner', 'compact', 'scoreboard'] as const;
export type StatusStyle = (typeof STATUS_STYLES)[number];

export const STATUS_STYLE_LABELS: Record<StatusStyle, string> = {
	banner: 'Banner: map art, a column of players per faction',
	compact: 'Compact: map thumbnail, faction counts and the top three',
	scoreboard: 'Scoreboard: ranked table of everyone across the factions'
};

export const isStatusStyle = (v: unknown): v is StatusStyle =>
	typeof v === 'string' && (STATUS_STYLES as readonly string[]).includes(v);
