// Configs written before the game's own path suffix was known hold Url=<origin>/api/feed/events,
// so those servers post here. Same handler; nothing to rewrite or restart on the host.
export { POST } from '../../../../../ingest/events/+server';
