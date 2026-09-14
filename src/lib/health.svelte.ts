import { untrack } from 'svelte';
import type { LiveView } from './types';

/** Last known reachability per server id, shown as the pulse dot in the switcher and dashboard. */
export const health = $state<Record<string, boolean>>({});
/** Servers whose listener asked the panel to slow down (429 with Retry-After), and until when. */
export const throttled = $state<Record<string, string | null>>({});
/** What the worker knows about each server's build, as the live stream reports it. */
export const identity = $state<
	Record<string, { build: string; gameServerId: string; startedAt: string | null }>
>({});

export function setHealth(id: string, ok: boolean) {
	// untrack: an $effect that calls this must not depend on the value it is about to overwrite,
	// or every poll-driven change would re-run it and restore the old value.
	if (untrack(() => health[id]) !== ok) health[id] = ok;
}

const expiry = new Map<string, ReturnType<typeof setTimeout>>();

/** Every live view the browser receives passes through here, so headers need no stream of their own. */
export function noteLive(v: LiveView) {
	setHealth(v.serverId, v.ok);
	const until =
		v.throttledUntil && Date.parse(v.throttledUntil) > Date.now() ? v.throttledUntil : null;
	if (untrack(() => throttled[v.serverId]) !== until) throttled[v.serverId] = until;
	// A hold ends on its own; without another event the indicator would otherwise stay amber.
	clearTimeout(expiry.get(v.serverId));
	if (until)
		expiry.set(
			v.serverId,
			setTimeout(
				() => {
					if (throttled[v.serverId] === until) throttled[v.serverId] = null;
				},
				Math.max(0, Date.parse(until) - Date.now())
			)
		);
	if (v.build || v.gameServerId || v.startedAt) {
		const cur = untrack(() => identity[v.serverId]);
		if (
			!cur ||
			cur.build !== v.build ||
			cur.gameServerId !== v.gameServerId ||
			cur.startedAt !== v.startedAt
		)
			identity[v.serverId] = {
				build: v.build,
				gameServerId: v.gameServerId,
				startedAt: v.startedAt
			};
	}
}
