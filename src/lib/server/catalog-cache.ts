// The per-server catalog and feature flags the server layout needs on every page: they change
// with plugin builds, not with matches, so one read per server per hour per web process. A
// connection test forgets the entry so a new build is picked up at once.
import type { Catalog, Features } from '$lib/types';

const CATALOG_TTL_MS = 3600_000;

export interface CatalogEntry {
	catalog: Catalog;
	features: Features;
}

const cache = new Map<string, CatalogEntry & { until: number }>();

export function cachedCatalog(serverId: string): CatalogEntry | null {
	const hit = cache.get(serverId);
	return hit && hit.until > Date.now() ? hit : null;
}

export function rememberCatalog(serverId: string, entry: CatalogEntry): void {
	cache.set(serverId, { ...entry, until: Date.now() + CATALOG_TTL_MS });
}

export function forgetCatalog(serverId: string): void {
	cache.delete(serverId);
}
