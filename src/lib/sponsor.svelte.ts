import { rconGet } from './api';

/** The banner URL each server advertises (GET /v1/sponsor), shared by the header and the config page. */
export const sponsor = $state<Record<string, string>>({});

const inflight = new Map<string, Promise<void>>();

/** Reads the banner once per server unless forced; a failed read leaves the last value. */
export function loadSponsor(id: string, force = false): Promise<void> {
	if (!force && id in sponsor) return Promise.resolve();
	const running = inflight.get(id);
	if (running && !force) return running;
	const p = rconGet<{ imageUrl: string }>(id, 'sponsor')
		.then((s) => {
			sponsor[id] = s.imageUrl || '';
		})
		.catch(() => {
			if (!(id in sponsor)) sponsor[id] = '';
		})
		.finally(() => {
			if (inflight.get(id) === p) inflight.delete(id);
		});
	inflight.set(id, p);
	return p;
}
