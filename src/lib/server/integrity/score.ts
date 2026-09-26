/** Explainable Integrity score. This is separate from Warcon's legacy advisory risk score. */
export interface IntegrityRuleConfig {
	committeeKpmMinutes: number;
	kpmBands: { min: number; points: number }[];
	uniqueVictimBands: { min: number; points: number }[];
	reportBands: { min: number; points: number }[];
	repeatWindowMinutes: number;
	repeatSecond: number;
	repeatThird: number;
	repeatBoth5: number;
	repeatBoth6: number;
	repeatKo: number;
	repeatKoWindowHours: number;
	steamPriorCap: number;
	recentVac: number;
	recentGameBan: number;
	oldVac: number;
	oldGameBan: number;
	lowPlaytimeHours: number;
	lowPlaytimeKpm: number;
	lowPlaytimePoints: number;
	headshotMinKills: number;
	headshotMinPct: number;
	headshotMax: number;
	penetrationMinKills: number;
	penetrationMinPct: number;
	penetrationMax: number;
	burstMax: number;
	burstFindingMin: number;
	passiveWatchThreshold: number;
	activeWatchThreshold: number;
	koThreshold: number;
	quarantineThreshold: number;
	quarantineDays: number;
	minimumOnlineForAutoAction: number;
	mode: 'dry_run' | 'enforce';
}

export const DEFAULT_INTEGRITY_RULES: IntegrityRuleConfig = {
	committeeKpmMinutes: 3,
	kpmBands: [
		{ min: 4, points: 18 },
		{ min: 4.5, points: 24 },
		{ min: 5, points: 32 },
		{ min: 6, points: 42 },
		{ min: 8, points: 52 }
	],
	uniqueVictimBands: [
		{ min: 8, points: 3 },
		{ min: 12, points: 6 },
		{ min: 18, points: 10 }
	],
	reportBands: [
		{ min: 1, points: 1 },
		{ min: 3, points: 4 },
		{ min: 5, points: 8 },
		{ min: 8, points: 12 },
		{ min: 12, points: 16 }
	],
	repeatWindowMinutes: 15,
	repeatSecond: 8,
	repeatThird: 14,
	repeatBoth5: 8,
	repeatBoth6: 14,
	repeatKo: 10,
	repeatKoWindowHours: 24,
	steamPriorCap: 15,
	recentVac: 8,
	recentGameBan: 12,
	oldVac: 1,
	oldGameBan: 2,
	lowPlaytimeHours: 2,
	lowPlaytimeKpm: 6,
	lowPlaytimePoints: 5,
	headshotMinKills: 15,
	headshotMinPct: 70,
	headshotMax: 10,
	penetrationMinKills: 15,
	penetrationMinPct: 50,
	penetrationMax: 6,
	burstMax: 20,
	burstFindingMin: 7,
	passiveWatchThreshold: 20,
	activeWatchThreshold: 40,
	koThreshold: 54,
	quarantineThreshold: 64,
	quarantineDays: 365,
	minimumOnlineForAutoAction: 20,
	mode: 'dry_run'
};

export interface IntegritySignals {
	committeeMode?: boolean;
	behaviorReasons: BehaviorReason[];
	kpm180: number;
	uniqueVictims: number;
	/** Independent abnormal findings in the last repeatWindowMinutes, newest first. */
	previousKpm: number[];
	uniqueReporters: number;
	repeatHighRiskWindow: boolean;
	infantryKills: number;
	headshots: number;
	penetrations: number;
	burstPoints: number;
	vacBans: number;
	gameBans: number;
	daysSinceLastBan: number | null;
	/** null means Private, failed or otherwise unknown. */
	wardogsPlaytimeHours: number | null;
}

export type BehaviorReason = 'kpm' | 'headshot' | 'penetration' | 'burst';

export interface RiskComponent {
	code: string;
	points: number;
	detail: string;
}

export type IntegrityLevel =
	| 'NORMAL'
	| 'PASSIVE_WATCH'
	| 'ACTIVE_WATCH'
	| 'AUTO_KO'
	| 'AUTO_QUARANTINE_ELIGIBLE'
	| 'AUTO_QUARANTINE_24H'
	| 'AUTO_QUARANTINE_7D';

export interface IntegrityScore {
	score: number;
	level: IntegrityLevel;
	breakdown: RiskComponent[];
	currentBehaviorAnomaly: boolean;
}

const tier = (value: number, bands: readonly { min: number; points: number }[]): number =>
	bands.reduce((points, band) => (value >= band.min ? Math.max(points, band.points) : points), 0);

/** Never uses profile privacy, names or the legacy advisory risk score. */
export function scoreIntegrity(
	signals: IntegritySignals,
	config: IntegrityRuleConfig = DEFAULT_INTEGRITY_RULES
): IntegrityScore {
	const breakdown: RiskComponent[] = [];
	const add = (code: string, points: number, detail: string) => {
		if (points > 0) breakdown.push({ code, points, detail });
	};
	const kpmPoints = signals.committeeMode
		? signals.kpm180 > 2
			? 6
			: 0
		: tier(signals.kpm180, config.kpmBands);
	add(
		signals.committeeMode ? 'committee_kpm_watch' : 'infantry_kpm_180',
		kpmPoints,
		`${signals.kpm180.toFixed(2)} infantry KPM`
	);
	if (kpmPoints && !signals.committeeMode) {
		add(
			'unique_victims',
			tier(signals.uniqueVictims, config.uniqueVictimBands),
			`${signals.uniqueVictims} unique victims`
		);
		const prior = signals.previousKpm.filter((kpm) => kpm >= config.kpmBands[0].min);
		add(
			'repeat_window',
			prior.length >= 2 ? config.repeatThird : prior.length === 1 ? config.repeatSecond : 0,
			`${prior.length + 1} independent abnormal windows`
		);
		if (prior.some((kpm) => kpm >= 6) && signals.kpm180 >= 6)
			add('repeat_extreme', config.repeatBoth6, 'Two windows at 6+ KPM');
		else if (prior.some((kpm) => kpm >= 5) && signals.kpm180 >= 5)
			add('repeat_extreme', config.repeatBoth5, 'Two windows at 5+ KPM');
	}
	add(
		'unique_reports',
		tier(signals.uniqueReporters, config.reportBands),
		`${signals.uniqueReporters} unique reporters`
	);
	add(
		'repeat_high_risk_window',
		signals.repeatHighRiskWindow ? config.repeatKo : 0,
		'Another high-risk window in the review period'
	);
	if (signals.infantryKills >= config.headshotMinKills)
		add(
			'headshots',
			signals.headshots / signals.infantryKills >= config.headshotMinPct / 100
				? config.headshotMax
				: 0,
			`${signals.headshots}/${signals.infantryKills} infantry headshots`
		);
	if (signals.infantryKills >= config.penetrationMinKills)
		add(
			'penetrations',
			signals.penetrations / signals.infantryKills >= config.penetrationMinPct / 100
				? config.penetrationMax
				: 0,
			`${signals.penetrations}/${signals.infantryKills} infantry penetrations`
		);
	add(
		'kill_burst',
		Math.min(config.burstMax, Math.max(0, signals.burstPoints)),
		'Server-feed kill burst'
	);
	const banAge = signals.daysSinceLastBan;
	const recent = banAge !== null && banAge <= 365;
	const steamPrior = Math.min(
		config.steamPriorCap,
		Math.max(0, signals.vacBans) * (recent ? config.recentVac : config.oldVac) +
			Math.max(0, signals.gameBans) * (recent ? config.recentGameBan : config.oldGameBan)
	);
	add('steam_ban_prior', steamPrior, 'Public Steam ban history, time-decayed');
	if (
		signals.wardogsPlaytimeHours !== null &&
		signals.wardogsPlaytimeHours < config.lowPlaytimeHours &&
		signals.kpm180 >= config.lowPlaytimeKpm
	)
		add('low_playtime', config.lowPlaytimePoints, 'Low public WARDOGS playtime with extreme KPM');
	const score = Math.min(
		100,
		breakdown.reduce((sum, item) => sum + item.points, 0)
	);
	const level = integrityLevel(score, config);
	return {
		score,
		level,
		breakdown,
		currentBehaviorAnomaly:
			signals.behaviorReasons.length > 0 || (!!signals.committeeMode && signals.kpm180 > 2)
	};
}

export function integrityLevel(score: number, config: IntegrityRuleConfig): IntegrityLevel {
	return score >= config.quarantineThreshold
		? 'AUTO_QUARANTINE_ELIGIBLE'
		: score >= config.koThreshold
			? 'AUTO_KO'
			: score >= config.activeWatchThreshold
				? 'ACTIVE_WATCH'
				: score >= config.passiveWatchThreshold
					? 'PASSIVE_WATCH'
					: 'NORMAL';
}
