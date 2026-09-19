// The organisation's ban message: the text a banned player is shown, built from the ban's reason
// and a few facts about it. Shared by the sync (what goes to the game) and the forms (the preview).

/** The message that sends the reason alone, as before there was a template. */
export const DEFAULT_BAN_MESSAGE = '{reason}';

export const BAN_MESSAGE_VARS = [
	'reason',
	'duration',
	'expires',
	'banned',
	'uid',
	'admin'
] as const;
export type BanMessageVar = (typeof BAN_MESSAGE_VARS)[number];

/** The template's own length, and the most the rendered text may run to (the reason's own cap). */
export const MAX_BAN_MESSAGE = 200;

export interface BanFacts {
	/** the list entry's id; the uid is cut from it */
	entryId: string;
	reason: string;
	addedByName: string;
	addedAt: Date;
	expiresAt: Date | null;
}

/** A short id for a ban, stable for the life of its entry: enough to find it on the ban list. */
export const banUid = (entryId: string): string =>
	`B-${entryId
		.replace(/[^a-z0-9]/gi, '')
		.slice(0, 6)
		.toUpperCase()}`;

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const two = (n: number) => String(n).padStart(2, '0');

/** 26 Sep 2026 09:12 UTC: the game shows the text to anyone anywhere, so no local time. */
const stamp = (d: Date): string =>
	`${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())} UTC`;

/** Perm, 7d, 36h or 45m: the length of the ban as it was set. */
function duration(from: Date, to: Date | null): string {
	if (!to) return 'Perm';
	const mins = Math.max(1, Math.round((to.getTime() - from.getTime()) / 60_000));
	if (mins % 1440 === 0) return `${mins / 1440}d`;
	if (mins >= 60) return `${Math.round(mins / 60)}h`;
	return `${mins}m`;
}

export function banMessageVars(f: BanFacts): Record<BanMessageVar, string> {
	return {
		reason: f.reason,
		duration: duration(f.addedAt, f.expiresAt),
		expires: f.expiresAt ? stamp(f.expiresAt) : 'never',
		banned: stamp(f.addedAt),
		uid: banUid(f.entryId),
		admin: f.addedByName
	};
}

/** The placeholders in a template that are not ones a ban message knows. */
export const unknownBanVars = (template: string): string[] => [
	...new Set(
		[...template.matchAll(/\{([a-z_]+)\}/gi)]
			.map((m) => m[1].toLowerCase())
			.filter((k) => !(BAN_MESSAGE_VARS as readonly string[]).includes(k))
	)
];

/**
 * The text for one ban. Separators left dangling by an empty value (a ban with no reason) are
 * trimmed off the ends, so "{reason} | Expires {expires}" does not open with a bar.
 */
export function renderBanMessage(template: string, f: BanFacts): string {
	const vars: Record<string, string> = banMessageVars(f);
	return (template || DEFAULT_BAN_MESSAGE)
		.replace(/\{([a-z_]+)\}/gi, (m, key: string) => vars[key.toLowerCase()] ?? m)
		.replace(/^[\s|·:,;-]+|[\s|·:,;-]+$/g, '')
		.slice(0, MAX_BAN_MESSAGE);
}
