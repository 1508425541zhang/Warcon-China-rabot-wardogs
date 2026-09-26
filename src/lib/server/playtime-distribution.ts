export type PlaytimeGroup = { minutes: number | null; state: string; count: number };
export function summarizePlaytime(groups: PlaytimeGroup[], enabled: boolean) {
	const known = groups
		.filter(
			(g) =>
				g.state === 'known' &&
				g.minutes !== null &&
				Number.isSafeInteger(g.minutes) &&
				g.minutes >= 0
		)
		.sort((a, b) => a.minutes! - b.minutes!);
	const total = groups.reduce((s, g) => s + g.count, 0);
	const available = known.reduce((s, g) => s + g.count, 0);
	const unknown = total - available;
	const maxHours = known.length ? known[known.length - 1].minutes! / 60 : 0;
	const widths = [
		1, 2, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000, 100000, 1000000, 10000000
	];
	const width = widths.find((w) => maxHours / w < 20) ?? 10000000;
	const bins = available
		? Array.from({ length: Math.floor(maxHours / width) + 1 }, (_, i) => ({
				from: i * width,
				to: (i + 1) * width,
				count: 0,
				percent: 0,
				cumulative: 0
			}))
		: [];
	let seen = 0,
		p80Minutes: number | null = null;
	for (const g of known) {
		bins[Math.floor(g.minutes! / 60 / width)].count += g.count;
		seen += g.count;
		if (p80Minutes === null && seen >= Math.ceil(available * 0.8)) p80Minutes = g.minutes;
	}
	seen = 0;
	for (const b of bins) {
		seen += b.count;
		b.percent = (b.count / available) * 100;
		b.cumulative = (seen / available) * 100;
	}
	return {
		enabled,
		total,
		available,
		unknown,
		p80Minutes,
		bins,
		pending: groups.filter((g) => g.state === 'pending').reduce((s, g) => s + g.count, 0),
		errors: groups.filter((g) => g.state === 'error').reduce((s, g) => s + g.count, 0)
	};
}
export type PlaytimeDistribution = ReturnType<typeof summarizePlaytime>;
