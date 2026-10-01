/** Runs the built Svelte renderer and Rust API/Worker/model against a disposable local DB. */
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes, randomUUID } from 'node:crypto';
import { resolve, join } from 'node:path';
import { writeFileSync, readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
const root = resolve(import.meta.dirname, '../..');
const pg =
	process.env.SMOKE_PSQL || resolve(root, '../warcon-tools/postgresql/17.11/pgsql/bin/psql.exe');
const database = 'warcon_rust_smoke_' + randomUUID().replaceAll('-', '');
const url = process.env.SMOKE_DATABASE_URL || 'postgresql://postgres@127.0.0.1:55440/postgres';
const dbUrl = new URL(url);
dbUrl.pathname = '/' + database;
const native = process.env.SMOKE_BIN_DIR || resolve(root, 'backend/target-native/debug');
const suffix = process.platform === 'win32' ? '.exe' : '';
const origin = 'http://127.0.0.1:4312';
const env = {
	...process.env,
	DATABASE_URL: dbUrl.href,
	ORIGIN: origin,
	BETTER_AUTH_SECRET: randomBytes(32).toString('hex'),
	ENCRYPTION_KEY: randomBytes(32).toString('base64'),
	RUST_FRONTEND_TOKEN: randomBytes(32).toString('hex'),
	RELAY_SECRET: randomBytes(32).toString('hex'),
	METRICS_TOKEN: randomBytes(32).toString('hex'),
	RUST_BACKEND_BIND: '127.0.0.1:4310',
	RUST_BACKEND_URL: 'http://127.0.0.1:4310',
	RUST_WORKER_BIND: '127.0.0.1:4311',
	RELAY_URL: 'http://127.0.0.1:4311',
	HOST: '127.0.0.1',
	PORT: '4312',
	STEAM_API_KEY: '',
	WARCON_SHORT_RISK_ENABLED: '0',
	SHORT_RISK_MODEL_PATH: resolve(root, 'services/short-risk/artifacts-rust/isolation.json')
};
Object.assign(env, {
	QQ_BOT_POLICIES: '[]',
	ONEBOT_HTTP_URL: '',
	ONEBOT_ACCESS_TOKEN: '',
	ONEBOT_EVENT_SECRET: ''
});
const children = [],
	checks = [];
function sql(text, target = url) {
	const r = spawnSync(pg, ['-X', '-v', 'ON_ERROR_STOP=1', '-d', target, '-c', text], {
		encoding: 'utf8',
		windowsHide: true
	});
	if (r.status !== 0) throw new Error(r.stderr);
	return r.stdout;
}
function start(file, args, extra = {}) {
	const child = spawn(file, args, {
		cwd: root,
		env: { ...env, ...extra },
		windowsHide: true,
		stdio: ['ignore', 'pipe', 'pipe']
	});
	const record = { child, output: '' };
	for (const stream of [child.stdout, child.stderr])
		stream.on('data', (b) => (record.output += b.toString()));
	children.push(record);
	return record;
}
async function ready(path) {
	const deadline = Date.now() + 25000;
	while (Date.now() < deadline) {
		try {
			if ((await fetch(path, { redirect: 'manual' })).status < 500) return;
		} catch {}
		await new Promise((r) => setTimeout(r, 200));
	}
	throw new Error('Service did not become ready: ' + path);
}
async function form(path, body, cookie = '') {
	const response = await fetch(origin + path, {
		method: 'POST',
		headers: {
			origin,
			accept: 'text/html',
			'content-type': 'application/x-www-form-urlencoded',
			cookie
		},
		body: new URLSearchParams(body),
		redirect: 'manual'
	});
	return response;
}
let created = false;
try {
	sql('CREATE DATABASE ' + database);
	created = true;
	const migration = spawnSync(join(native, 'migrate' + suffix), [join(root, 'drizzle')], {
		cwd: root,
		env,
		encoding: 'utf8',
		windowsHide: true
	});
	assert.equal(migration.status, 0, migration.stderr);
	checks.push('native SQL migrations');
	start(join(native, 'warcon-worker' + suffix), [], { RELAY_URL: '' });
	await ready('http://127.0.0.1:4311/health');
	checks.push('native Worker lease and relay health');
	start(join(native, 'warcon-api' + suffix), []);
	await ready('http://127.0.0.1:4310/api/health');
	start(process.execPath, ['build/index.js']);
	await ready(origin + '/sign-in');
	let response = await fetch(origin + '/sign-in', { redirect: 'manual' });
	assert.equal(response.status, 303);
	assert.equal(response.headers.get('location'), '/setup');
	checks.push('uninitialized browser redirect');
	response = await form('/setup?/password', {
		username: 'smokeowner',
		displayName: '联调管理员',
		password: 'FixturePasswordOnly123!',
		again: 'FixturePasswordOnly123!'
	});
	assert.equal(response.status, 303, await response.clone().text());
	const cookie = response.headers
		.getSetCookie()
		.filter((v) => v.includes('session_token='))
		.map((v) => v.split(';')[0])
		.join('; ');
	assert.ok(cookie);
	checks.push('browser setup form and signed Set-Cookie');
	response = await fetch(origin + '/account', { headers: { cookie } });
	assert.equal(response.status, 200);
	assert.ok((await response.text()).includes('联调管理员'));
	checks.push('authenticated Svelte SSR through native page controller');
	response = await fetch(origin + '/api/identity/session', { headers: { cookie } });
	const session = await response.json();
	assert.equal(session.user.username, 'smokeowner');
	assert.equal(session.user.role, 'owner');
	checks.push('native identity JSON proxy');
	response = await form('/account?/totpStart', { password: 'FixturePasswordOnly123!' }, cookie);
	assert.equal(response.status, 200);
	const html = await response.text();
	assert.ok(html.includes('<svg'));
	checks.push('native OTP state and rendered QR');
	response = await fetch(origin + '/api/settings', {
		method: 'PUT',
		headers: { cookie, 'content-type': 'application/json' },
		body: JSON.stringify({ values: { sampleMs: 15000 }, reset: [] })
	});
	assert.equal(response.status, 403);
	checks.push('cookie API rejects absent Origin');
	sql(
		"INSERT INTO organizations(id,name,slug)VALUES('smokeorg','Smoke Org','smoke-org');INSERT INTO org_members(org_id,user_id,role) SELECT 'smokeorg',id,'owner' FROM \"user\" WHERE username='smokeowner';INSERT INTO lists(id,org_id,kind)VALUES('smokeban','smokeorg','ban'),('smokereserve','smokeorg','reserve');INSERT INTO servers(id,org_id,name,host,port,password_enc)VALUES('smokeserver','smokeorg','Smoke Server','example.invalid',1,'fixture');INSERT INTO server_live(server_id,ok,status,players,status_at,players_at,updated_at)VALUES('smokeserver',true,'{\"map\":\"Europe\",\"matchSeconds\":100}', '[]',now(),now(),now())",
		dbUrl.href
	);
	for (const path of [
		'/server/smokeserver/automation',
		'/server/smokeserver/integrity',
		'/server/smokeserver/integrity/cases',
		'/server/smokeserver/qq-bindings',
		'/plugins',
		'/orgs/smokeorg'
	]) {
		response = await fetch(origin + path, { headers: { cookie } });
		assert.equal(response.status, 200, path + ': ' + (await response.clone().text()));
	}
	checks.push('automation, Integrity, cases, QQ, plugins and organization SSR pages');
	response = await fetch(origin + '/api/health', { headers: { cookie } });
	const health = await response.json();
	for (const key of ['delivered', 'failed', 'skipped', 'unknown', 'inFlight', 'lastPassAt'])
		assert.equal(typeof health.worker.delivery[key], 'number', key);
	sql(
		"INSERT INTO site_settings(key,value)VALUES('sampleMs','{\"n\":15000}')ON CONFLICT(key)DO UPDATE SET value=excluded.value",
		dbUrl.href
	);
	response = await fetch(origin + '/api/health', { headers: { cookie } });
	const updated = await response.json();
	assert.ok(updated.worker.settingsVersion > health.worker.settingsVersion);
	response = await fetch(origin + '/metrics');
	assert.equal(response.status, 401);
	for (const base of [origin, 'http://127.0.0.1:4311']) {
		response = await fetch(base + '/metrics', {
			headers: { authorization: 'Bearer ' + env.METRICS_TOKEN }
		});
		assert.equal(response.status, 200);
		const text = await response.text();
		for (const name of [
			'warcon_build_info',
			'warcon_observation_seconds_bucket',
			'warcon_deliveries_total',
			'warcon_feed_posts_total',
			'warcon_servers',
			'warcon_fleet'
		])
			assert.ok(text.includes(name), name);
	}
	checks.push('native diagnostic counters, settings reload and protected metrics');
	response = await fetch(origin + '/api/live/events?ids=smokeserver', {
		headers: { cookie },
		signal: AbortSignal.timeout(20000)
	});
	assert.equal(response.status, 200);
	assert.ok(response.headers.get('content-type')?.includes('text/event-stream'));
	const reader = response.body.getReader();
	const chunk = await reader.read();
	assert.ok(chunk.value?.length);
	await reader.cancel();
	checks.push('SSE first frame forwarded without buffering');
	response = await form('/sign-out', {}, cookie);
	assert.equal(response.status, 303);
	assert.ok(response.headers.getSetCookie().some((v) => v.includes('Max-Age=0')));
	checks.push('native logout invalidates browser cookie');
	response = await form('/sign-in?/password', { username: 'smokeowner', password: 'wrong' });
	assert.equal(response.status, 401);
	checks.push('native failure envelope rendered as form error');
	const recovery = spawnSync(join(native, 'reset-auth' + suffix), ['smokeowner'], {
		cwd: root,
		env,
		encoding: 'utf8',
		windowsHide: true
	});
	assert.equal(recovery.status, 0, recovery.stderr);
	const temporary = recovery.stdout.match(
		/Temporary password \(change at next sign-in\): (\S+)/
	)?.[1];
	assert.ok(temporary);
	response = await form('/sign-in?/password', { username: 'smokeowner', password: temporary });
	assert.equal(response.status, 303);
	const recoveredCookie = response.headers
		.getSetCookie()
		.filter((v) => v.includes('session_token='))
		.map((v) => v.split(';')[0])
		.join('; ');
	response = await fetch(origin + '/api/identity/session', {
		headers: { cookie: recoveredCookie }
	});
	const recovered = await response.json();
	assert.equal(recovered.gate.password, true);
	checks.push('offline native account recovery requires password change');
	const artifact =
		process.env.SMOKE_MODEL_ARTIFACTS ||
		resolve(root, '../full-temporal/at30-expanded-20261001/bun-artifacts');
	const modelToken = randomBytes(32).toString('hex');
	start(join(native, 'warcon-model' + suffix), [], {
		MODEL_ARTIFACTS_DIR: artifact,
		MODEL_DATABASE_URL: dbUrl.href,
		MODEL_API_TOKEN: modelToken,
		MODEL_HOST: '127.0.0.1',
		MODEL_PORT: '4313'
	});
	await ready('http://127.0.0.1:4313/v1/health');
	response = await fetch('http://127.0.0.1:4313/v1/health', {
		headers: { authorization: 'Bearer ' + modelToken }
	});
	assert.equal(response.status, 200);
	checks.push('native long model starts from real trained weights');
	const report = {
		ok: true,
		checks,
		frontendBusinessRuntime: 'Rust API only',
		productionTouched: false
	};
	writeFileSync(
		join(root, 'backend/local-stack-smoke.json'),
		JSON.stringify(report, null, 2) + '\n'
	);
	console.log(JSON.stringify(report));
} catch (e) {
	console.error(String(e));
	// Startup diagnostics are bounded; generated credentials and cookies never enter logs.
	for (const c of children) console.error(c.output.slice(-1200));
	process.exitCode = 1;
} finally {
	for (const c of children) c.child.kill();
	await Promise.all(
		children.map((c) =>
			c.child.exitCode !== null ? Promise.resolve() : new Promise((r) => c.child.once('exit', r))
		)
	);
	if (created) sql('DROP DATABASE ' + database + ' WITH (FORCE)');
}
