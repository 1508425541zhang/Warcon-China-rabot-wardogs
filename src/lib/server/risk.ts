// Advisory risk, never proof of cheating. Steam bans/friends and local match statistics are
// evidence with different coverage; missing data contributes nothing except explicit privacy.

export interface RiskProfile {
	error?: string;
	public: boolean;
	accountCreatedAt: Date | null;
	vacBans: number;
	gameBans: number;
	daysSinceLastBan: number | null;
	communityBanned: boolean;
	economyBan: string;
	friendsState?: string;
	friendsTotal?: number;
	friendsChecked?: number;
	bannedFriends?: number;
}

export interface RiskPerformance {
	matches: number;
	wins: number;
	losses: number;
	draws: number;
	kills: number;
	deaths: number;
	feedKills: number;
	headshots: number;
}

export interface RiskSignals {
	/** null when Steam has not been asked (no key, or not fetched yet) */
	profile: RiskProfile | null;
	steamEnabled: boolean;
	watched: { reason: string } | null;
	/** bans on other servers in the same org */
	bannedOn: { serverName: string; reason: string }[];
	/** banned players whose last known name looks like this one */
	resembles: { name: string; steamId: string; serverName: string }[];
	performance?: RiskPerformance | null;
	now?: Date;
}

export interface RiskReason {
	code: string;
	text: string;
	weight: number;
}
export type RiskLevel = 'low' | 'medium' | 'high';
export interface Risk {
	score: number;
	level: RiskLevel;
	reasons: RiskReason[];
	/** false when Steam signals were unavailable, so a "low" here means "nothing local" */
	steamChecked: boolean;
}

export const RISK_HIGH = 50;
export const RISK_MEDIUM = 20;

const DAY = 86400_000;

/** Whole days since a date, or null. */
export const accountAgeDays = (createdAt: Date | null, now = new Date()): number | null =>
	createdAt ? Math.max(0, Math.floor((now.getTime() - createdAt.getTime()) / DAY)) : null;

export function assessRisk(s: RiskSignals): Risk {
	const now = s.now ?? new Date();
	const reasons: RiskReason[] = [];
	const p = s.profile && !s.profile.error ? s.profile : null;
	if (p) {
		const banAge = p.daysSinceLastBan;
		const banWeight = (base: number) =>
			banAge === null
				? base
				: banAge < 30
					? base + 15
					: banAge < 365
						? base + 8
						: banAge < 365 * 3
							? base
							: Math.max(5, base - 15);
		if (p.vacBans > 0) {
			reasons.push({
				code: 'vac',
				text: `${p.vacBans} VAC ban${p.vacBans === 1 ? '' : 's'}${
					p.daysSinceLastBan !== null ? `, last ${p.daysSinceLastBan} days ago` : ''
				}`,
				weight: Math.min(70, banWeight(45) + Math.min(10, (p.vacBans - 1) * 5))
			});
		}
		if (p.gameBans > 0)
			reasons.push({
				code: 'gameban',
				text: `${p.gameBans} game ban${p.gameBans === 1 ? '' : 's'}${banAge === null ? '' : `, last ${banAge} days ago`}`,
				weight: Math.min(45, banWeight(25) + Math.min(5, (p.gameBans - 1) * 5))
			});
		if (p.communityBanned)
			reasons.push({ code: 'community', text: 'Steam community ban', weight: 10 });
		if (p.economyBan && p.economyBan !== 'none')
			reasons.push({ code: 'economy', text: `Steam economy ban (${p.economyBan})`, weight: 5 });
		if (!p.public) reasons.push({ code: 'private', text: 'Steam profile is private', weight: 12 });
		const age = accountAgeDays(p.accountCreatedAt, now);
		if (age !== null && age < 7) {
			reasons.push({
				code: 'age',
				text: `Steam account is ${age} day${age === 1 ? '' : 's'} old`,
				weight: 30
			});
		} else if (age !== null && age < 30) {
			reasons.push({ code: 'age', text: `Steam account is ${age} days old`, weight: 20 });
		} else if (age !== null && age < 90) {
			reasons.push({ code: 'age', text: `Steam account is ${age} days old`, weight: 10 });
		}
		if (p.friendsState === 'private')
			reasons.push({ code: 'friends_private', text: 'Steam friends list is private', weight: 8 });
		const bannedFriends = p.bannedFriends ?? 0;
		if (bannedFriends > 0)
			reasons.push({
				code: 'banned_friends',
				text: `${bannedFriends} banned Steam friend${bannedFriends === 1 ? '' : 's'} among ${p.friendsChecked ?? 0} checked${p.friendsState === 'partial' ? ` of ${p.friendsTotal ?? 0}` : ''}`,
				weight: Math.min(30, 6 + bannedFriends * 5)
			});
	}
	const perf = s.performance;
	if (perf) {
		const decided = perf.wins + perf.losses + perf.draws;
		const winRate = decided ? perf.wins / decided : 0;
		if (decided >= 20 && winRate >= 0.8)
			reasons.push({
				code: 'win_rate',
				text: `${Math.round(winRate * 100)}% wins across ${decided} recorded matches`,
				weight: 8
			});
		const kd = perf.deaths ? perf.kills / perf.deaths : perf.kills;
		if (perf.kills >= 100 && perf.deaths >= 20 && kd >= 4)
			reasons.push({
				code: 'kd',
				text: `${kd.toFixed(1)} K/D across ${perf.kills} kills and ${perf.deaths} deaths`,
				weight: 10
			});
		const headshotRate = perf.feedKills ? perf.headshots / perf.feedKills : 0;
		if (perf.feedKills >= 50 && headshotRate >= 0.6)
			reasons.push({
				code: 'headshots',
				text: `${Math.round(headshotRate * 100)}% headshots across ${perf.feedKills} kill-feed kills`,
				weight: 10
			});
	}
	for (const b of s.bannedOn)
		reasons.push({
			code: 'banned_elsewhere',
			text: `Banned on ${b.serverName}${b.reason ? `: ${b.reason}` : ''}`,
			weight: 60
		});
	for (const r of s.resembles.slice(0, 3))
		reasons.push({
			code: 'resembles',
			text: `Name resembles banned ${r.name} (${r.steamId}) on ${r.serverName}`,
			weight: 20
		});
	if (s.watched)
		reasons.push({
			code: 'watchlist',
			text: `On the watchlist${s.watched.reason ? `: ${s.watched.reason}` : ''}`,
			weight: 15
		});
	const score = Math.max(
		0,
		Math.min(
			100,
			reasons.reduce((n, r) => n + r.weight, 0)
		)
	);
	return {
		score,
		level: score >= RISK_HIGH ? 'high' : score >= RISK_MEDIUM ? 'medium' : 'low',
		reasons,
		steamChecked: !!p
	};
}

// ---- name resemblance ---------------------------------------------------------------------------

const LEET: Record<string, string> = {
	'0': 'o',
	'1': 'i',
	'3': 'e',
	'4': 'a',
	'5': 's',
	'7': 't',
	'8': 'b',
	'@': 'a',
	$: 's',
	'!': 'i',
	'|': 'l'
};

/** Lower-case letters only, with the usual leetspeak substitutions undone. */
export function normaliseName(name: string): string {
	return String(name || '')
		.toLowerCase()
		.normalize('NFKD')
		.replace(/[0134578@$!|]/g, (c) => LEET[c] ?? c)
		.replace(/[^a-z]/g, '');
}

export function levenshtein(a: string, b: string): number {
	if (a === b) return 0;
	if (!a.length) return b.length;
	if (!b.length) return a.length;
	let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
	for (let i = 1; i <= a.length; i++) {
		const cur = [i];
		for (let j = 1; j <= b.length; j++) {
			cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
		}
		prev = cur;
	}
	return prev[b.length];
}

/**
 * Do two player names look like the same person? Equal after normalisation, one containing the
 * other (5+ letters), or within one edit (6+ letters) / two edits (10+ letters).
 */
export function namesResemble(a: string, b: string): boolean {
	const x = normaliseName(a);
	const y = normaliseName(b);
	const shorter = Math.min(x.length, y.length);
	if (shorter < 4) return false;
	if (x === y) return true;
	if (shorter >= 5 && (x.includes(y) || y.includes(x))) return true;
	const d = levenshtein(x, y);
	return (shorter >= 6 && d <= 1) || (shorter >= 10 && d <= 2);
}
