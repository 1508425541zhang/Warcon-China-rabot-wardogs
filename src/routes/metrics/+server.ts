// Prometheus scrape target of the web process (and, with WARCON_ROLE=all, the worker's figures
// too). Bearer METRICS_TOKEN; 404 while it is unset. The worker process serves the same path on
// its own port (src/worker/runtime.ts).
import type { RequestHandler } from './$types';
import { getEnv } from '$lib/server/env';
import { metricsResponse } from '$lib/server/metrics';

export const GET: RequestHandler = ({ request }) =>
	metricsResponse(request, getEnv().METRICS_TOKEN);
