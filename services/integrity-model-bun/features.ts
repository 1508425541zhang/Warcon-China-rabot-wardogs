export const masks = [
	'cash_observed',
	'cash_change_valid',
	'combat_change_valid',
	'history_kpm_observed',
	'window_kpm_observed',
	'headshot_rate_observed',
	'penetration_rate_observed',
	'max15_observed',
	'unique_victims_observed',
	'burst_observed',
	'interval_observed',
	'roster_observed',
	'active_observed'
];
export const maskFor = [0, 1, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
export type Row = Record<string, number | string | null>;
type ObjectRow = Record<string, unknown>;
type Sample = {
	elapsed: number;
	cash: number | null;
	kills: number | null;
	deaths: number | null;
	roster: number | null;
};
type Metric = { elapsed: number; [key: string]: number | null };
type Group = { progress: Sample[]; history: Metric[]; windows: Metric[] };
const tables = [
	'matches',
	'player_progress_samples',
	'integrity_player_metric_history',
	'integrity_windows'
];
const numeric = (v: unknown): number | null =>
	v === null || v === undefined || v === '' || !Number.isFinite(Number(v)) ? null : Number(v);
const integer = (v: unknown) => (numeric(v) === null ? null : Math.trunc(numeric(v)!));
const object = (v: unknown): ObjectRow => {
	if (!v || typeof v !== 'object' || Array.isArray(v)) throw new Error('Object required');
	return v as ObjectRow;
};
function time(v: unknown) {
	if (v === undefined || v === null || v === '') return null;
	if (typeof v !== 'string' || !/(Z|[+-]\d\d:\d\d)$/.test(v))
		throw new Error('UTC timestamp required');
	const t = Date.parse(v);
	if (!Number.isFinite(t)) throw new Error('Invalid timestamp');
	return t;
}
const mean = (values: (number | null)[]) => {
	const xs = values.filter((v): v is number => v !== null);
	return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null;
};
const total = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
const iso = (value: number) =>
	new Date(value)
		.toISOString()
		.replace(/\.000Z$/, '+00:00')
		.replace(/\.(\d{3})Z$/, '.$1000+00:00');
const maximum = (xs: (number | null)[]) => {
	const ns = xs.filter((v): v is number => v !== null);
	return ns.length ? Math.max(...ns) : null;
};

/** Same four-table, match-aligned 30-second reconstruction as the published Python adapter. */
export function buildRows(input: unknown): Row[] {
	const request = object(input);
	if (request.schema !== 'warcon-raw-30s-v1') throw new Error('Feature schema mismatch');
	const source = object(request.sources);
	if (Object.keys(source).sort().join(',') !== [...tables].sort().join(','))
		throw new Error('Invalid source tables');
	for (const table of tables)
		if (!Array.isArray(source[table]) || (source[table] as unknown[]).length > 25000)
			throw new Error('Bounded source arrays required');
	const matches = source.matches as unknown[];
	if (matches.length !== 1) throw new Error('One match required');
	const match = object(matches[0]),
		start = time(match.started_at),
		end = time(match.ended_at);
	if (start === null) return [];
	if (end !== null && end <= start) return [];
	const mid = String(match.id),
		duration = end === null ? Infinity : (end - start) / 1000;
	const groups = new Map<string, Group>();
	function group(server: unknown, player: unknown) {
		const key = JSON.stringify([String(server || ''), String(player)]);
		if (!groups.has(key)) groups.set(key, { progress: [], history: [], windows: [] });
		return groups.get(key)!;
	}
	function elapsed(at: unknown) {
		const t = time(at);
		if (t === null) return null;
		const value = (t - start!) / 1000;
		return value < -5 || value >= duration ? null : Math.max(0, value);
	}
	for (const value of source.player_progress_samples as unknown[]) {
		const row = object(value),
			at = elapsed(row.observed_at);
		if (at === null || String(row.match_id || '') !== mid) continue;
		const players = Array.isArray(row.players) ? row.players : [];
		for (const value of players) {
			if (!value || typeof value !== 'object' || Array.isArray(value)) continue;
			const p = value as ObjectRow;
			if (!p.steamId) continue;
			group(row.server_id, p.steamId).progress.push({
				elapsed: at,
				cash: numeric(p.cash),
				kills: integer(p.kills),
				deaths: integer(p.deaths),
				roster: numeric(row.roster_size === undefined ? players.length : row.roster_size)
			});
		}
	}
	for (const table of ['integrity_player_metric_history', 'integrity_windows']) {
		for (const value of source[table] as unknown[]) {
			const row = object(value),
				at = elapsed(row.observed_at);
			if (
				at === null ||
				/:match:(\d+)$/.exec(String(row.round_id || ''))?.[1] !== mid ||
				!row.steam_id
			)
				continue;
			const g = group(row.server_id, row.steam_id);
			if (table === 'integrity_player_metric_history')
				g.history.push({ elapsed: at, kpm: numeric(row.kpm_180) });
			else
				g.windows.push({
					elapsed: at,
					inf: numeric(row.infantry_kills),
					kpm: numeric(row.kpm_180),
					victims: numeric(row.unique_victims),
					head: numeric(row.headshots),
					pen: numeric(row.penetrations),
					burst: numeric(row.burst_points),
					max15: numeric(row.max_kills_15s),
					interval: numeric(row.median_kill_interval)
				});
		}
	}
	const results: Row[][] = [];
	for (const g of groups.values()) {
		const pr = g.progress.sort((a, b) => a.elapsed - b.elapsed);
		const spans: [number, number][] = [];
		for (let i = 0; i < pr.length; i++) {
			const t = pr[i].elapsed,
				next = i + 1 < pr.length ? pr[i + 1].elapsed : t + 30;
			spans.push([
				t,
				Math.max(t, Math.min(next - t > 0 && next - t <= 90 ? next : t + 30, duration))
			]);
		}
		for (const m of [...g.history, ...g.windows]) spans.push([m.elapsed, m.elapsed + 1]);
		spans.sort((a, b) => a[0] - b[0]);
		const merged: [number, number][] = [];
		for (const span of spans) {
			const last = merged.at(-1);
			if (last && span[0] <= last[1]) last[1] = Math.max(last[1], span[1]);
			else merged.push([...span]);
		}
		if (!merged.length) continue;
		const hi = Math.min(
			Math.max(...merged.map(([a, b]) => Math.floor(Math.max(a, b - 1e-9) / 30))),
			Math.floor(duration / 30) - 1
		);
		const lo = Math.max(0, Math.min(...merged.map(([a]) => Math.floor(a / 30))), hi - 200);
		function buckets<T extends { elapsed: number }>(samples: T[]) {
			const result = new Map<number, T[]>();
			for (const s of samples) {
				const n = Math.floor(s.elapsed / 30);
				if (!result.has(n)) result.set(n, []);
				result.get(n)!.push(s);
			}
			return result;
		}
		const progress = buckets(pr),
			history = buckets(g.history),
			windows = buckets(g.windows);
		const rows: Row[] = [];
		let previous: Sample | undefined;
		for (let bucket = lo; bucket <= hi; bucket++) {
			const ps = progress.get(bucket) || [],
				hs = history.get(bucket) || [],
				ws = windows.get(bucket) || [];
			const p = ps.at(-1),
				left = bucket * 30,
				right = left + 30;
			let active = Math.min(
				30,
				total(merged.map(([a, b]) => Math.max(0, Math.min(right, b, duration) - Math.max(left, a))))
			);
			const observed = !!(ps.length || hs.length || ws.length);
			if (observed && active <= 0) active = 1;
			const row: Row = {
				bucket_start_utc: iso(start + left * 1000),
				is_active: Number(active > 0 || observed),
				bucket_observed: Number(observed),
				cash_balance: p?.cash ?? null,
				cash_observed: Number(p !== undefined && p.cash !== null),
				roster_size: p?.roster ?? null,
				roster_observed: Number(p !== undefined && p.roster !== null),
				active_fraction: Number((active / 30).toFixed(6)),
				active_observed: 1,
				cash_change_30s: null,
				cash_change_valid: 0,
				kills_change_30s: null,
				deaths_change_30s: null,
				combat_change_valid: 0
			};
			if (
				p &&
				previous &&
				p.elapsed - previous.elapsed >= 0 &&
				p.elapsed - previous.elapsed <= 45
			) {
				if (p.cash !== null && previous.cash !== null) {
					row.cash_change_30s = p.cash - previous.cash;
					row.cash_change_valid = 1;
				}
				if (
					p.kills !== null &&
					p.deaths !== null &&
					previous.kills !== null &&
					previous.deaths !== null &&
					p.kills >= previous.kills &&
					p.deaths >= previous.deaths
				) {
					row.kills_change_30s = p.kills - previous.kills;
					row.deaths_change_30s = p.deaths - previous.deaths;
					row.combat_change_valid = 1;
				}
			}
			row.kpm_180_history_mean = mean(hs.map((s) => s.kpm));
			row.history_kpm_observed = Number(row.kpm_180_history_mean !== null);
			row.kpm_180_window_mean = mean(ws.map((s) => s.kpm));
			row.window_kpm_observed = Number(row.kpm_180_window_mean !== null);
			const inf = ws.map((s) => s.inf).filter((v): v is number => v !== null && v > 0);
			for (const [channel, field, mask] of [
				['headshot_rate_window', 'head', 'headshot_rate_observed'],
				['penetration_rate_window', 'pen', 'penetration_rate_observed']
			]) {
				const values = ws.map((s) => s[field]).filter((v): v is number => v !== null);
				const rate = inf.length && values.length ? total(values) / total(inf) : null;
				row[channel] = rate !== null && rate >= 0 && rate <= 1 ? rate : null;
				row[mask] = Number(row[channel] !== null);
			}
			for (const [channel, field, mask] of [
				['max_kills_15s_max', 'max15', 'max15_observed'],
				['unique_victims_window_max', 'victims', 'unique_victims_observed'],
				['burst_points_window_max', 'burst', 'burst_observed']
			]) {
				row[channel] = maximum(ws.map((s) => s[field]));
				row[mask] = Number(row[channel] !== null);
			}
			row.median_kill_interval_s =
				ws
					.map((s) => s.interval)
					.filter((v) => v !== null)
					.at(-1) ?? null;
			row.interval_observed = Number(row.median_kill_interval_s !== null);
			rows.push(row);
			previous = p;
		}
		if (rows.length) results.push(rows.slice(-200));
	}
	if (results.length > 1) throw new Error('One player per request required');
	return results[0] || [];
}
