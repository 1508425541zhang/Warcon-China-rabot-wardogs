// Browser-side Steam persona lookup for lists that only hold SteamIDs (bans, reserved slots, an
// id being typed). Answers are cached for the page's life; once the panel says lookup is not
// configured, nothing is asked again.
import { api, ApiError } from './api';

export interface SteamProfile {
	name: string;
	avatar: string;
}

export const isSteamId = (v: string) => /^\d{17}$/.test(v);

const cache = new Map<string, SteamProfile | null>();
let disabled = false;
/** ids a request is out for, so two pages asking at once share one round trip */
const pending = new Set<string>();
let lastBatch: Promise<void> = Promise.resolve();

/** true once the panel has answered that no Steam key is configured */
export const steamLookupDisabled = () => disabled;

/**
 * Personas for the given ids, from the cache where possible. Ids the lookup could not answer
 * (no key, Steam down) are left out so the caller shows the bare id and asks again later.
 */
export async function steamProfiles(ids: string[]): Promise<Record<string, SteamProfile | null>> {
	const out: Record<string, SteamProfile | null> = {};
	const missing: string[] = [];
	for (const id of new Set(ids.filter(isSteamId))) {
		if (cache.has(id)) out[id] = cache.get(id)!;
		else missing.push(id);
	}
	if (disabled || !missing.length) return out;
	if (missing.some((id) => pending.has(id))) {
		await lastBatch;
		return steamProfiles(ids);
	}
	for (const id of missing) pending.add(id);
	lastBatch = fetchBatches(missing).finally(() => {
		for (const id of missing) pending.delete(id);
	});
	await lastBatch;
	for (const id of missing) if (cache.has(id)) out[id] = cache.get(id)!;
	return out;
}

async function fetchBatches(ids: string[]): Promise<void> {
	for (let i = 0; i < ids.length; i += 100) {
		const batch = ids.slice(i, i + 100);
		try {
			const res = await api<Record<string, SteamProfile | null>>(
				'GET',
				`/api/steam/profiles?ids=${batch.join(',')}`
			);
			for (const id of batch) cache.set(id, res[id] ?? null);
		} catch (err) {
			if (err instanceof ApiError && err.code === 'steam_disabled') disabled = true;
			return;
		}
	}
}
