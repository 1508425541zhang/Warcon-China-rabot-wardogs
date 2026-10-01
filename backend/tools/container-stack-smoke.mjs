/** Exercise a new, isolated Compose database through the built Node renderer and Rust services. */
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
const root = resolve(import.meta.dirname, '../..');
const origin = process.env.CONTAINER_SMOKE_ORIGIN || 'http://localhost:4302';
const docker = process.env.SMOKE_DOCKER || 'docker';
const checks = [];
const password = randomBytes(24).toString('hex');
const compose = [
	'compose',
	'--project-name',
	'warcon-rust-validation',
	'--env-file',
	'.env.rust.dev',
	'-f',
	'compose.rust-dev.yml'
];
if (process.env.CONTAINER_SMOKE_NO_OVERRIDE !== '1')
	compose.push('-f', 'backend/artifacts-local/compose.validation.yml');
function sql(text) {
	const result = spawnSync(
		docker,
		[
			...compose,
			'exec',
			'-T',
			'db',
			'psql',
			'-X',
			'-v',
			'ON_ERROR_STOP=1',
			'-U',
			'warcon',
			'-d',
			'warcon',
			'-At',
			'-c',
			text
		],
		{ cwd: root, encoding: 'utf8', windowsHide: true }
	);
	assert.equal(result.status, 0, result.stderr);
	return result.stdout.trim();
}
async function form(path, fields, cookie = '') {
	return fetch(origin + path, {
		method: 'POST',
		headers: {
			origin,
			cookie,
			'content-type': 'application/x-www-form-urlencoded',
			accept: 'text/html'
		},
		body: new URLSearchParams(fields),
		redirect: 'manual'
	});
}
async function api(path, method = 'GET', body, cookie = '') {
	const response = await fetch(origin + path, {
		method,
		headers: { origin, cookie, 'content-type': 'application/json' },
		body: body === undefined ? undefined : JSON.stringify(body),
		redirect: 'manual'
	});
	const data = await response.json();
	assert.ok(
		response.status >= 200 && response.status < 300,
		`${method} ${path}: ${response.status} ${JSON.stringify(data)}`
	);
	return data;
}
function cookies(response) {
	return response.headers
		.getSetCookie()
		.filter((v) => v.includes('session_token='))
		.map((v) => v.split(';')[0])
		.join('; ');
}
try {
	assert.equal(
		sql('SELECT count(*) FROM "user"'),
		'0',
		'Only an empty validation database may be initialized'
	);
	let response = await fetch(origin + '/setup');
	assert.equal(response.status, 200);
	checks.push('fresh container setup page');
	response = await form('/setup?/password', {
		username: 'containerowner',
		displayName: 'Rust 容器验证',
		password,
		again: password
	});
	assert.equal(response.status, 303, await response.clone().text());
	const cookie = cookies(response);
	assert.ok(cookie);
	checks.push('browser initialization and native session cookie');
	assert.equal(
		(await api('/api/identity/session', 'GET', undefined, cookie)).user.username,
		'containerowner'
	);
	response = await form('/account?/totpStart', { password }, cookie);
	assert.equal(response.status, 200);
	assert.ok((await response.text()).includes('<svg'));
	checks.push('native OTP and frontend QR rendering');
	const org = await api('/api/orgs', 'POST', { name: 'Container validation' }, cookie);
	const server = await api(
		'/api/servers',
		'POST',
		{ orgId: org.id, name: 'Native demo', host: 'demo', port: 7776, password: 'demo' },
		cookie
	);
	assert.ok(server.id);
	checks.push('native organization and demo server API');
	// Only this isolated fixture receives a demo Feed token; no production configuration is touched.
	assert.match(server.id, /^[a-zA-Z0-9_-]+$/);
	sql(`UPDATE servers SET feed_token_hash='container-fixture' WHERE id='${server.id}'`);
	for (const path of [
		`/server/${server.id}`,
		`/server/${server.id}/automation`,
		`/server/${server.id}/integrity`,
		`/server/${server.id}/integrity/cases`,
		`/server/${server.id}/qq-bindings`,
		'/plugins',
		`/orgs/${org.id}`
	]) {
		response = await fetch(origin + path, { headers: { cookie } });
		assert.equal(response.status, 200, path + ': ' + (await response.clone().text()));
	}
	checks.push('server, automation, Integrity, cases, QQ, plugins and org SSR');
	response = await fetch(origin + `/api/live/events?ids=${server.id}`, {
		headers: { cookie },
		signal: AbortSignal.timeout(20000)
	});
	assert.equal(response.status, 200);
	assert.ok(response.headers.get('content-type').includes('text/event-stream'));
	const reader = response.body.getReader();
	assert.ok((await reader.read()).value.length);
	await reader.cancel();
	checks.push('native relay SSE streams through Node');
	const deadline = Date.now() + 90000;
	let live = false,
		kills = 0;
	while (Date.now() < deadline) {
		live =
			Number(
				sql(
					`SELECT count(*) FROM server_live WHERE server_id='${server.id}' AND ok AND jsonb_array_length(players)>0`
				)
			) > 0;
		kills = Number(sql(`SELECT count(*) FROM kills WHERE server_id='${server.id}'`));
		if (live && kills > 0) break;
		await new Promise((r) => setTimeout(r, 1000));
	}
	assert.ok(live, 'Native worker did not observe demo players');
	assert.ok(kills > 0, 'Native demo Feed did not persist kills');
	const consumers = sql(
		`SELECT string_agg(consumer,',' ORDER BY consumer) FROM (SELECT DISTINCT consumer FROM feed_processing_jobs WHERE server_id='${server.id}') c`
	);
	assert.equal(consumers, 'integrity,legacy');
	checks.push('worker polling, raw kills and both independent consumers');
	const health = await api('/api/health', 'GET', undefined, cookie);
	assert.ok(health.worker.enabled);
	checks.push('native API and worker health');
	const model = spawnSync(
		docker,
		[
			...compose,
			'exec',
			'-T',
			'model',
			'warcon-health',
			'http://127.0.0.1:8091/v1/health',
			'--model'
		],
		{ cwd: root, encoding: 'utf8', windowsHide: true }
	);
	assert.equal(model.status, 0, model.stderr);
	checks.push('real long model container with Bearer health');
	response = await form('/sign-out', {}, cookie);
	assert.equal(response.status, 303);
	assert.ok(response.headers.getSetCookie().some((v) => v.includes('Max-Age=0')));
	checks.push('native logout clears browser cookie');
	const report = { ok: true, checks, kills, productionTouched: false };
	writeFileSync(
		resolve(root, 'backend/container-stack-smoke.json'),
		JSON.stringify(report, null, 2) + '\n'
	);
	console.log(JSON.stringify(report));
} catch (e) {
	console.error(String(e));
	process.exitCode = 1;
}
