// What happens in the worker once the kill feed has written a batch: the kills go out on the
// event bus (the SSE route fans them to browsers; in the split roles every web process gets them
// through the relay stream), and the rules that watch kills get their turn.
import type { Env } from './env';
import { emit } from './events';
import type { KillView } from '$lib/types';

export async function onKillsIngested(
	env: Env,
	serverId: string,
	kills: KillView[]
): Promise<void> {
	if (!kills.length) return;
	emit({ type: 'kills', serverId, kills });
	void env;
}
