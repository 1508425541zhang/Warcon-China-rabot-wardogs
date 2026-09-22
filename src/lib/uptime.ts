// How long the game process has been up, and where that sits against the game's scheduled
// restart. A WARDOGS server restarts once its uptime passes 24 hours (fixed in the game, not
// a setting), but not on the mark: the restart happens when the round in progress ends. So past
// the threshold the server is "restarting after this round", and before it there is a window
// that opens in so many minutes. The worker derives `startedAt` from `uptimeSeconds` on
// GET /v1/health; the header, the status cards and (later) the rules read this.

/** Hours of uptime after which WARDOGS restarts the server at the end of the round. Hard-coded in the game. */
export const RESTART_AFTER_HOURS = 24;

export interface RestartWindow {
	/** milliseconds since the game process started */
	upMs: number;
	/** the threshold has passed: the server restarts when the current round ends */
	due: boolean;
	/** milliseconds until the threshold; null when due, or when no restart is scheduled */
	untilDueMs: number | null;
}

/** Shown as "restart window in …" once the threshold is this close. */
export const RESTART_SOON_MS = 60 * 60_000;

/** null when the start time is unknown. `restartAfterHours` 0 means no restart on uptime (tests). */
export function restartWindow(
	startedAt: string | null | undefined,
	restartAfterHours: number,
	now: number
): RestartWindow | null {
	if (!startedAt) return null;
	const started = Date.parse(startedAt);
	if (!Number.isFinite(started)) return null;
	const upMs = Math.max(0, now - started);
	if (!(restartAfterHours > 0)) return { upMs, due: false, untilDueMs: null };
	const untilDueMs = restartAfterHours * 3600_000 - upMs;
	return untilDueMs <= 0 ? { upMs, due: true, untilDueMs: null } : { upMs, due: false, untilDueMs };
}

/** "3d 2h", "9h 12m", "12m", "<1m": the two largest units that are non-zero, minutes at least. */
export function fmtUptime(ms: number): string {
	const mins = Math.floor(Math.max(0, ms) / 60_000);
	if (mins < 1) return '<1m';
	const d = Math.floor(mins / 1440);
	const h = Math.floor((mins % 1440) / 60);
	const m = mins % 60;
	if (d > 0) return h > 0 ? `${d}d ${h}h` : `${d}d`;
	if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
	return `${m}m`;
}
