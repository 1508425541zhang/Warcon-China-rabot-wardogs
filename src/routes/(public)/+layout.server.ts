// The public route group: no session, no redirect. Each page checks its own server; the shell
// shows what the page reports as `heading`.
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async () => ({});
