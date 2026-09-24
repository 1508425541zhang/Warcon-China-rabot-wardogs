// Score-tick payout bands. The game pays a control zone and a hot zone a fixed amount per tick at
// the slowest setting and scales it down in bands as the tick gets faster; the rates and bands
// are the ones the official console shows beside its Tick Timer slider (js/config-editor.js,
// deploy of 2026-09-14).
export const TICK_FULL_RATE = { control: 200, hot: 400 } as const;

export interface TickBand {
	/** the slowest tick this band starts at, in seconds */
	from: number;
	multiplier: number;
	label: string;
}

export const TICK_BANDS: readonly TickBand[] = [
	{ from: 30, multiplier: 1, label: '30s+' },
	{ from: 27, multiplier: 0.9, label: '27–29s' },
	{ from: 24, multiplier: 0.8, label: '24–26s' },
	{ from: 21, multiplier: 0.7, label: '21–23s' },
	{ from: 0, multiplier: 0.6, label: '0–20s' }
];

export function tickBand(seconds: number | null | undefined): TickBand | null {
	const s = Number(seconds);
	if (seconds === null || seconds === undefined || !Number.isFinite(s)) return null;
	return TICK_BANDS.find((b) => s >= b.from) ?? TICK_BANDS[TICK_BANDS.length - 1];
}

export interface TickReward {
	control: number;
	hot: number;
	band: TickBand;
}

/** What one tick pays each zone at this cadence, or null when the cadence is unknown. */
export function tickReward(seconds: number | null | undefined): TickReward | null {
	const band = tickBand(seconds);
	if (!band) return null;
	return {
		control: Math.round(TICK_FULL_RATE.control * band.multiplier),
		hot: Math.round(TICK_FULL_RATE.hot * band.multiplier),
		band
	};
}
