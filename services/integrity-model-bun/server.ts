import { createHash, timingSafeEqual } from 'node:crypto';
import { Predictor } from './inference';
import { ExpandedPredictor } from './expanded-inference';
import { expandedSourceReader } from './expanded-db';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const MAX_BODY = 8 * 1024 * 1024;
export function serve(
	predictor: Predictor | ExpandedPredictor,
	token: string,
	port = 8091,
	hostname = '127.0.0.1'
) {
	if (token.length < 32 || /\s/.test(token))
		throw new Error('MODEL_API_TOKEN must contain at least 32 characters without whitespace');
	const expected = createHash('sha256')
		.update('Bearer ' + token)
		.digest();
	const reply = (status: number, body: unknown) =>
		Response.json(body, { status, headers: { 'cache-control': 'no-store' } });
	let busy = false;
	return Bun.serve({
		hostname,
		port,
		maxRequestBodySize: MAX_BODY,
		idleTimeout: 30,
		async fetch(request) {
			if (
				!timingSafeEqual(
					expected,
					createHash('sha256')
						.update(request.headers.get('authorization') || '')
						.digest()
				)
			)
				return reply(401, { error: 'Unauthorized' });
			const path = new URL(request.url).pathname;
			if (request.method === 'GET' && path === '/v1/health')
				return reply(200, { ok: true, ...predictor.manifest });
			if (request.method !== 'POST' || path !== '/v1/assess')
				return reply(404, { error: 'Not found' });
			if (busy) return reply(503, { error: 'Inference busy' });
			busy = true;
			try {
				const bytes = await request.arrayBuffer();
				if (!bytes.byteLength || bytes.byteLength > MAX_BODY)
					return reply(413, { error: 'Invalid body size' });
				let body: unknown;
				try {
					body = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
				} catch {
					return reply(400, { error: 'Invalid JSON' });
				}
				try {
					return reply(200, await predictor.assess(body));
				} catch {
					return reply(400, { error: 'Invalid inference input' });
				}
			} finally {
				busy = false;
			}
		},
		error() {
			return reply(500, { error: 'Inference failed' });
		}
	});
}
if (import.meta.main) {
	const root = process.env.MODEL_ARTIFACTS_DIR || join(import.meta.dir, 'artifacts-30m');
	const metadata = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
	const predictor =
		metadata.channels > 27
			? new ExpandedPredictor(root, expandedSourceReader(process.env.MODEL_DATABASE_URL || ''))
			: new Predictor(root);
	const server = serve(
		predictor,
		process.env.MODEL_API_TOKEN || '',
		Number(process.env.MODEL_PORT || 8091),
		process.env.MODEL_HOST || '127.0.0.1'
	);
	console.log(
		JSON.stringify({
			ready: true,
			model: predictor.manifest.model_id,
			runtime: 'bun',
			host: server.hostname,
			port: server.port
		})
	);
}
