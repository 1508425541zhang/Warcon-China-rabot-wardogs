/** Frozen training feature semantics; thirty-second snapshots of one player in one match. */
export const finite = (v: any): v is number => typeof v === 'number' && Number.isFinite(v);
const ts = (v: any) => (v instanceof Date ? v.getTime() / 1000 : Date.parse(v) / 1000);
const mean = (v: number[]) => (v.length ? v.reduce((a, b) => a + b, 0) / v.length : NaN);
const quantile = (v: number[], q: number) => {
	if (!v.length) return NaN;
	const s = [...v].sort((a, b) => a - b),
		p = (s.length - 1) * q;
	return s[Math.floor(p)] + (s[Math.ceil(p)] - s[Math.floor(p)]) * (p % 1);
};
const flatten = (v: any, p = ''): Record<string, number> => {
	const o: Record<string, number> = {};
	if (v && typeof v === 'object' && !Array.isArray(v))
		for (const [k, x] of Object.entries(v)) {
			const n = p ? p + '.' + k : k;
			if (finite(x)) o[n] = x;
			else if (x && typeof x === 'object' && !Array.isArray(x)) Object.assign(o, flatten(x, n));
		}
	return o;
};
const classes = [
	'automatic',
	'sniper',
	'shotgun',
	'other_infantry',
	'vehicle',
	'fixed_weapon',
	'unknown'
];
const arms = new Set(
	[
		'AK74M',
		'Mosin',
		'MP9',
		'WEPN_029',
		'M4',
		'M500',
		'MP43',
		'SKS',
		'SVDM',
		'KH2002',
		'TAR21',
		'A91',
		'SV98',
		'MK22',
		'Glock17',
		'CombatBow'
	].map((w) => 'Id.Item.' + w)
);
export function normalizeRows(rows: number[][], contract: any) {
	const f = contract.features.length,
		input = new Float32Array(rows.length * f * 2);
	for (let t = 0; t < rows.length; t++)
		for (let i = 0; i < f; i++) {
			const v = rows[t][i];
			if (finite(v)) {
				input[t * f * 2 + i] = Math.max(
					-100,
					Math.min(100, (Math.fround(v) - contract.center[i]) / contract.scale[i])
				);
				input[t * f * 2 + f + i] = 1;
			}
		}
	return input;
}
export function buildExpanded(source: any, contract: any) {
	const match = source.match,
		player = String(source.player),
		start = ts(match.started_at),
		end = Math.floor(ts(source.end) / 30) * 30;
	if (!finite(start) || !finite(end) || end - start < 1800) return [];
	const raw = new Map<string, any>();
	const feed: number[] = [];
	for (const b of source.batches ?? []) {
		feed.push(ts(b.received_at));
		for (const e of b.payload?.events ?? []) raw.set(String(b.instance_id) + '/' + e.eventId, e);
	}
	feed.sort((a, b) => a - b);
	const seen = new Set<string>();
	const events = (source.kills ?? [])
		.filter((e: any) => {
			const key = e.instance_id + '/' + e.event_id;
			if (
				seen.has(key) ||
				!finite(e.event_time) ||
				e.event_time < 0 ||
				String(e.match_row) !== String(match.id)
			)
				return false;
			seen.add(key);
			return true;
		})
		.map((e: any) => {
			const r = raw.get(e.instance_id + '/' + e.event_id),
				tags = Array.isArray(r?.contextTags)
					? r.contextTags.map((s: string) => s.split('.').at(-1))
					: Array.isArray(e.tags)
						? e.tags
						: null;
			return {
				...e,
				received: ts(e.ts),
				clock: e.event_time,
				tags,
				headshot: r
					? Array.isArray(r.contextTags)
						? r.contextTags.some((s: string) => s.endsWith('.Headshot'))
						: null
					: e.headshot,
				raw: !!r
			};
		})
		.sort((a: any, b: any) => a.received - b.received);
	// A mixed boot/map cannot be safely treated as a continuous player sequence.
	if (
		new Set(events.map((e: any) => e.instance_id)).size > 1 ||
		new Set(events.map((e: any) => e.map)).size > 1
	)
		return [];
	const polls: any[] = [];
	const statuses: any[] = [];
	for (const o of source.observations ?? []) {
		const at = ts(o.received_at);
		if (o.endpoint === '/v1/players') {
			const ps = Array.isArray(o.payload) ? o.payload : (o.payload?.players ?? []);
			polls.push({ at, players: ps, rosterSize: o.payload?.roster_size });
		} else if (o.endpoint === '/v1/status') statuses.push({ at, state: o.payload });
	}
	for (const p of source.progress ?? [])
		polls.push({ at: ts(p.observed_at), players: p.players ?? [], rosterSize: p.roster_size });
	polls.sort((a, b) => a.at - b.at);
	statuses.sort((a, b) => a.at - b.at);
	const names = contract.base.features,
		weaponVocab = contract.base.weapon_vocabulary,
		weaponSet = new Set(weaponVocab);
	const numeric = names
		.filter((n: string) => n.startsWith('player_') && !n.startsWith('player_delta_'))
		.map((n: string) => n.slice(7));
	let pi = 0,
		ei = 0,
		si = 0,
		fi = 0,
		anchor: any = null,
		state: any = {},
		stateAt = -Infinity,
		obsAt = -Infinity,
		active = new Set<string>(),
		activeCount = 0;
	const fields = new Map<string, { at: number; value: any }>(),
		previous = new Map<string, { at: number; value: number }>(),
		output: number[][] = [];
	const attack = (e: any) =>
		e.killer_steam_id === player &&
		e.victim_steam_id !== player &&
		!e.suicide &&
		!e.team_kill &&
		!['Suicide', 'Falling', 'RoadKill'].some((t) => e.tags?.includes(t));
	const distance = (e: any) =>
		finite(e.distance_m) && e.distance_m >= 0 && e.distance_m <= 2000 && !e.distance_invalid;
	for (let tick = end - 1830; tick <= end; tick += 30) {
		while (pi < polls.length && polls[pi].at <= tick) {
			const p = polls[pi++];
			if (p.players.some((v: any) => 'kills' in v || 'pingMs' in v)) {
				active = new Set(p.players.map((v: any) => String(v.steamId)));
				activeCount = active.size;
			}
			if (finite(p.rosterSize)) activeCount = p.rosterSize;
			for (const v of p.players)
				if (String(v.steamId) === player) {
					for (const [k, value] of Object.entries(v)) fields.set(k, { at: p.at, value });
					obsAt = Math.max(obsAt, p.at);
				}
		}
		while (ei < events.length && events[ei].received <= tick) {
			const e = events[ei++];
			if (!anchor || e.clock >= anchor.clock) anchor = e;
		}
		while (si < statuses.length && statuses[si].at <= tick) {
			state = statuses[si].state;
			stateAt = statuses[si++].at;
		}
		while (fi < feed.length && feed[fi] <= tick) fi++;
		const feedAge = fi ? tick - feed[fi - 1] : Infinity,
			clock = anchor && tick - anchor.received <= 120 ? anchor.clock + tick - anchor.received : NaN;
		const recent = events
			.slice(0, ei)
			.filter(
				(e: any) =>
					tick - e.received <= 600 && (e.killer_steam_id === player || e.victim_steam_id === player)
			);
		const window = recent.filter(
				(e: any) => finite(clock) && clock - 120 < e.clock && e.clock <= clock
			),
			known = feedAge <= 45 || window.some((e: any) => tick - e.received < 30);
		const out: Record<string, number> = {};
		const attacks = window.filter(attack);
		const validCore = finite(clock) && known;
		const rates: number[] = [];
		for (const w of [60, 120]) {
			const es = window.filter((e: any) => e.clock > clock - w),
				ak = es.filter(attack),
				ar = ak
					.filter((e: any) => arms.has(e.cause))
					.sort(
						(a: any, b: any) =>
							a.clock - b.clock || String(a.event_id).localeCompare(String(b.event_id))
					);
			const h = ar
				.filter((e: any) => Array.isArray(e.tags) && typeof e.headshot === 'boolean')
				.map((e: any) => Number(e.headshot));
			const p = ar
				.filter((e: any) => Array.isArray(e.tags))
				.map((e: any) =>
					typeof e.penetration === 'boolean'
						? Number(e.penetration)
						: Number(e.tags.some((t: string) => t.includes('Penetration')))
				);
			const intervals = ar.slice(1).map((e: any, i: number) => e.clock - ar[i].clock);
			let burst = 0,
				left = 0;
			for (let right = 0; right < ar.length; right++) {
				while (ar[right].clock - ar[left].clock > 15) left++;
				burst = Math.max(burst, right - left + 1);
			}
			const ds = ar
				.filter((e: any) => distance(e) && contract.base.distance_baseline[e.cause])
				.map((e: any) => e.distance_m / contract.base.distance_baseline[e.cause].p95_m);
			const vals = [
				ar.length,
				ak.length,
				es.filter((e: any) => e.victim_steam_id === player).length,
				mean(h),
				mean(p),
				new Set(ar.map((e: any) => e.victim_steam_id)).size,
				burst,
				quantile(intervals, 0.5),
				quantile(ds, 0.9),
				h.length,
				ds.length
			];
			[
				'small_arm_kills',
				'kills',
				'deaths',
				'headshot_rate',
				'penetration_rate',
				'unique_victims',
				'max_kills_15s',
				'median_interval_s',
				'distance_ratio_p90',
				'headshot_samples',
				'distance_samples'
			].forEach((n, i) => (out[n + '_' + w + 's'] = validCore ? vals[i] : NaN));
			rates.push(ar.length);
		}
		out.kill_rate_change = validCore ? rates[0] - (rates[1] - rates[0]) : NaN;
		const arms120 = attacks.filter((e: any) => arms.has(e.cause)),
			wc: Record<string, number> = {};
		for (const e of arms120) wc[e.cause] = (wc[e.cause] ?? 0) + 1;
		out.dominant_weapon_fraction_120s =
			validCore && arms120.length ? Math.max(...Object.values(wc)) / arms120.length : NaN;
		const ds = attacks.filter(distance).map((e: any) => e.distance_m),
			heads = attacks
				.filter((e: any) => Array.isArray(e.tags) && typeof e.headshot === 'boolean')
				.map((e: any) => Number(e.headshot));
		Object.assign(out, {
			distance_mean_m_120s: mean(ds),
			distance_max_m_120s: ds.length ? Math.max(...ds) : NaN,
			all_headshot_rate_120s: mean(heads),
			all_headshot_samples_120s: known ? heads.length : NaN,
			raw_event_fraction_120s: window.length ? mean(window.map((e: any) => Number(e.raw))) : NaN,
			suicides_120s: known
				? window.filter((e: any) => e.suicide && e.victim_steam_id === player).length
				: NaN,
			teamkills_120s: known
				? window.filter((e: any) => e.team_kill && e.killer_steam_id === player).length
				: NaN,
			kill_clock_spread_120s: attacks.length
				? Math.max(...attacks.map((e: any) => e.clock)) -
					Math.min(...attacks.map((e: any) => e.clock))
				: NaN
		});
		for (const k of numeric) {
			const field = fields.get(k),
				v = field && tick - field.at <= 45 && finite(field.value) ? field.value : NaN,
				prior = previous.get(k);
			out['player_' + k] = v;
			out['player_delta_' + k] =
				prior && prior.at === tick - 30 && finite(v) && finite(prior.value) ? v - prior.value : NaN;
			previous.set(k, { at: tick, value: v });
		}
		const factionField = fields.get('faction'),
			faction = factionField && tick - factionField.at <= 45 ? factionField.value : null;
		const status = tick - stateAt <= 45 ? state : {};
		const scores: Record<string, number> = {};
		for (const s of status?.factionScores ?? status?.scores ?? [])
			if (finite(s.score)) scores[s.name] = s.score;
		const leading = Object.keys(scores).length ? Math.max(...Object.values(scores)) : NaN,
			own = scores[faction] ?? NaN;
		Object.assign(out, {
			roster_size: tick - obsAt <= 45 ? activeCount : NaN,
			own_faction_score: own,
			leading_faction_score: leading,
			faction_score_gap: leading - own,
			round_elapsed_s: tick - start,
			feed_age_s: finite(feedAge) ? feedAge : NaN,
			roster_age_s: finite(obsAt) ? tick - obsAt : NaN,
			utc_hour_sin: Math.sin(((tick % 86400) / 86400) * 2 * Math.PI),
			utc_hour_cos: Math.cos(((tick % 86400) / 86400) * 2 * Math.PI)
		});
		const flat = flatten(status);
		for (const n of names) if (n.startsWith('status:')) out[n] = flat[n.slice(7)] ?? NaN;
		const per = new Map<string, any[]>();
		for (const e of attacks) {
			const w = weaponSet.has(e.cause) ? e.cause : 'UNKNOWN';
			per.set(w, [...(per.get(w) ?? []), e]);
		}
		for (const w of weaponVocab) {
			const es = per.get(w) ?? [],
				h = es
					.filter((e: any) => typeof e.headshot === 'boolean' && Array.isArray(e.tags))
					.map((e: any) => Number(e.headshot));
			const d = es
				.filter((e: any) => distance(e) && contract.distance_baseline[w])
				.map((e: any) => e.distance_m / contract.distance_baseline[w].p95_m);
			out[`weapon:${w}:kills_120s`] = known ? es.length : NaN;
			out[`weapon:${w}:headshots_120s`] = h.length
				? h.reduce((a: number, b: number) => a + b, 0)
				: NaN;
			out[`weapon:${w}:headshot_rate_120s`] = mean(h);
			out[`weapon:${w}:mean_distance_ratio_120s`] = mean(d);
		}
		for (const n of names) {
			if (n.startsWith('tag_120s:'))
				out[n] = known ? attacks.filter((e: any) => e.tags?.includes(n.slice(9))).length : NaN;
			for (const prefix of ['faction', 'map', 'lighting'])
				if (n.startsWith(prefix + ':')) {
					const value =
						prefix === 'faction' ? faction : prefix === 'map' ? match.map : status?.lighting;
					const vocab = names
							.filter((x: string) => x.startsWith(prefix + ':'))
							.map((x: string) => x.slice(prefix.length + 1)),
						category = vocab.includes(value) ? value : 'UNKNOWN';
					out[n] = Number(n === prefix + ':' + category);
				}
			if (n.startsWith('experience:'))
				out[n] = Number((status?.experiences ?? []).includes(n.slice(11)));
		}
		// Apply the exact frozen class aggregation, after per-weapon float32 rounding.
		for (const k in out) out[k] = Math.fround(out[k]);
		for (const c of classes) {
			const ws = weaponVocab.filter((w: string) => contract.weapon_class_by_id[w] === c),
				ks = ws.map((w: string) => out[`weapon:${w}:kills_120s`]);
			const any = ks.some(finite);
			let count = 0,
				h = 0,
				hn = 0;
			const d: number[] = [];
			for (const w of ws) {
				const k = out[`weapon:${w}:kills_120s`],
					v = out[`weapon:${w}:headshots_120s`],
					dist = out[`weapon:${w}:mean_distance_ratio_120s`];
				if (finite(k)) count += k;
				if (finite(v)) {
					h += v;
					if (finite(k)) hn += k;
				}
				if (finite(dist)) d.push(dist);
			}
			Object.assign(out, {
				[`class:${c}:kills_120s`]: any ? count : NaN,
				[`class:${c}:headshots_observed_120s`]: any ? h : NaN,
				[`class:${c}:headshot_samples_120s`]: any ? hn : NaN,
				[`class:${c}:headshot_rate_120s`]: hn > 0 ? h / hn : NaN,
				[`class:${c}:max_weapon_mean_distance_ratio_120s`]: d.length ? Math.max(...d) : NaN
			});
		}
		const isCurrent = tick - obsAt <= 45 || recent.some((e: any) => tick - e.received <= 120);
		if (tick > end - 1800)
			output.push(
				isCurrent
					? contract.features.map((n: string) => Math.fround(out[n] ?? NaN))
					: Array(contract.features.length).fill(NaN)
			);
	}
	return output;
}
