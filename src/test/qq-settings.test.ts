import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld, type World } from './world';
import { callApi } from './call';
import type { Env } from '$lib/server/env';
import { GET, PUT, POST } from '../routes/api/admin/qq/+server';
import {
	qqSettingsView,
	saveQqSettings,
	loadQqSettings,
	testQqConnection
} from '$lib/server/qq/settings';
import { qqCredentials, qqPolicy, applyQqConfiguration } from '$lib/server/qq/config';
import { loadSettings } from '$lib/server/settings';
import { POST as webhook } from '../routes/api/qq/webhook/+server';
import { QqClient, qqSignature } from '$lib/server/qq/protocol';
import { processMessage } from '$lib/server/qq/runtime';

describe.skipIf(!hasTestDb)('site QQ configuration', () => {
	let env: Env;
	let world: World;
	const token = 'api-token-12345678901234567890';
	const secret = 'event-secret-1234567890123456';
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
	}, 120000);
	afterAll(async () => {
		await env.db.execute(sql`DELETE FROM site_settings WHERE key='qqCommunity'`);
		applyQqConfiguration(null);
	});
	const patch = (revision: string) => ({
		revision,
		enabled: true,
		url: 'http://127.0.0.1:3001',
		selfId: '123456',
		token,
		secret,
		policies: [
			{ serverId: world.server.id, groups: ['234567'], maps: ['mapA', 'mapB'], pointsPerMinute: 3 }
		]
	});
	test('only site owners can read, save or probe; organization owners and API keys cannot', async () => {
		for (const role of ['anon', 'member', 'owner', 'keyAll'] as const)
			for (const [handler, method] of [
				[GET, 'GET'],
				[PUT, 'PUT'],
				[POST, 'POST']
			] as const) {
				const result = await callApi(handler, world.users[role], {
					method,
					body: method === 'GET' ? undefined : {}
				});
				expect([401, 403]).toContain(result.status);
			}
	});
	test('saves encrypted secrets; response and audit contain no plaintext; runtime uses saved policy', async () => {
		const result = await callApi(PUT, world.users.site, {
			method: 'PUT',
			body: patch((await qqSettingsView(env)).revision)
		});
		expect(result.status).toBe(200);
		expect(JSON.stringify(result.body)).not.toContain(token);
		expect(JSON.stringify(result.body)).not.toContain(secret);
		const [row] = await env.db.execute(
			sql`SELECT value FROM site_settings WHERE key='qqCommunity'`
		);
		const stored = JSON.stringify(row.value);
		expect(stored).not.toContain(token);
		expect(stored).not.toContain(secret);
		expect(stored).toContain('v1.');
		expect(qqCredentials()?.token).toBe(token);
		expect(qqPolicy(world.server.id)?.pointsPerMinute).toBe(3);
		const logs = await env.db.execute(
			sql`SELECT detail FROM audit_log WHERE action='qq.settings.update'`
		);
		expect(JSON.stringify(logs)).not.toContain(token);
	});
	test('blank secret inputs retain credentials and stale edits are rejected', async () => {
		const old = await qqSettingsView(env);
		await saveQqSettings(
			env,
			{
				...patch(old.revision),
				token: '',
				secret: '',
				policies: old.policies.map((p) => ({ ...p, voteCost: 25 }))
			},
			world.users.site!.id
		);
		expect(qqCredentials()?.token).toBe(token);
		expect(qqPolicy(world.server.id)?.voteCost).toBe(25);
		await expect(saveQqSettings(env, patch(old.revision), world.users.site!.id)).rejects.toThrow(
			'其他管理员'
		);
	});
	test('invalid groups, unknown servers and clearing active secrets cannot change saved config', async () => {
		const before = await qqSettingsView(env);
		for (const change of [
			{ clearToken: true },
			{ policies: [{ ...before.policies[0], serverId: 'missing-server' }] },
			{ policies: [before.policies[0], { ...before.policies[0], serverId: world.otherServer.id }] },
			{ policies: [{ ...before.policies[0], pointsPerMinute: 0 }] }
		]) {
			await expect(
				saveQqSettings(env, { ...patch(before.revision), ...change }, world.users.site!.id)
			).rejects.toThrow();
		}
		expect((await qqSettingsView(env)).revision).toBe(before.revision);
	});
	test('global and server switches persist across worker refresh, and explicit clearing works', async () => {
		let view = await qqSettingsView(env);
		await saveQqSettings(
			env,
			{
				...patch(view.revision),
				policies: [
					{ ...view.policies[0], enabled: false },
					{ ...view.policies[0], serverId: world.otherServer.id, groups: ['345678'], enabled: true }
				]
			},
			world.users.site!.id
		);
		applyQqConfiguration(null);
		await loadSettings(env);
		expect(qqPolicy(world.server.id)).toBeUndefined();
		expect(qqPolicy(world.otherServer.id)).toBeDefined();
		view = await qqSettingsView(env);
		await saveQqSettings(
			env,
			{ ...patch(view.revision), enabled: false, clearToken: true, clearSecret: true },
			world.users.site!.id
		);
		applyQqConfiguration(null);
		await loadQqSettings(env);
		expect(qqCredentials()).toBeNull();
		expect(qqPolicy(world.server.id)).toBeUndefined();
		expect((await qqSettingsView(env)).hasToken).toBe(false);
	});
	test('connection probe checks the saved bot identity without sending any group message', async () => {
		await saveQqSettings(env, patch((await qqSettingsView(env)).revision), world.users.site!.id);
		const fetcher = (async (url: unknown, init?: RequestInit) => {
			expect(String(url)).toBe('http://127.0.0.1:3001/get_login_info');
			expect(new Headers(init?.headers).get('authorization')).toBe(`Bearer ${token}`);
			return Response.json({ status: 'ok', retcode: 0, data: { user_id: 123456 } });
		}) as typeof fetch;
		expect((await testQqConnection(env, fetcher)).ok).toBe(true);
		const wrong = (async () =>
			Response.json({
				status: 'ok',
				retcode: 0,
				data: { user_id: 999999 }
			})) as unknown as typeof fetch;
		await expect(testQqConnection(env, wrong)).rejects.toThrow('不匹配');
	});
	test('legacy settings default to NapCat; LLBot selection persists and cannot accept unknown providers', async () => {
		await env.db.execute(
			sql`UPDATE site_settings SET value=value-'provider' WHERE key='qqCommunity'`
		);
		expect((await qqSettingsView(env)).provider).toBe('napcat');
		let view = await qqSettingsView(env);
		await saveQqSettings(
			env,
			{ ...patch(view.revision), provider: 'llbot', url: 'http://127.0.0.1:3000' },
			world.users.site!.id
		);
		applyQqConfiguration(null);
		await loadQqSettings(env);
		view = await qqSettingsView(env);
		expect(view.provider).toBe('llbot');
		expect(qqCredentials()?.url).toBe('http://127.0.0.1:3000');
		expect(qqCredentials()?.token).toBe(token);
		await expect(
			saveQqSettings(env, { ...patch(view.revision), provider: 'unknown' }, world.users.site!.id)
		).rejects.toThrow();
		await saveQqSettings(
			env,
			{ ...patch(view.revision), token: '', secret: '' },
			world.users.site!.id
		);
		expect((await qqSettingsView(env)).provider).toBe('llbot');
		const fetcher = (async (_url: unknown) =>
			Response.json({ status: 'ok', retcode: 0, data: { user_id: 123456 } })) as typeof fetch;
		expect((await testQqConnection(env, fetcher)).message).toContain('LLBot');
	});
	test('LLBot signed array events deduplicate, reject bad authentication and reply through OneBot HTTP', async () => {
		await env.db.execute(sql`UPDATE qq_inbox SET state='done' WHERE state='pending'`);
		const raw = JSON.stringify({
			time: Math.floor(Date.now() / 1000),
			self_id: 123456,
			post_type: 'message',
			message_type: 'group',
			sub_type: 'normal',
			group_id: 234567,
			user_id: 345678,
			message_id: 901,
			message: [
				{ type: 'at', data: { qq: '123456' } },
				{ type: 'text', data: { text: ' /帮助' } }
			]
		});
		const post = (signature = qqSignature(secret, raw), selfId = '123456', body = raw) =>
			webhook({
				request: new Request('http://localhost/api/qq/webhook', {
					method: 'POST',
					headers: { 'x-self-id': selfId, 'x-signature': signature },
					body
				})
			} as Parameters<typeof webhook>[0]);
		expect((await post('bad')).status).toBe(401);
		expect((await post(undefined, '999999')).status).toBe(401);
		expect((await post(undefined, undefined, raw.replace('/帮助', '/积分'))).status).toBe(401);
		expect((await post()).status).toBe(204);
		expect((await post()).status).toBe(204);
		const id = 'ob11:123456:234567:901';
		expect(await env.db.execute(sql`SELECT id FROM qq_inbox WHERE id=${id}`)).toHaveLength(1);
		let sent = false;
		const fetcher = (async (url: unknown, init?: RequestInit) => {
			expect(String(url)).toBe('http://127.0.0.1:3000/send_group_msg');
			expect(new Headers(init?.headers).get('authorization')).toBe(`Bearer ${token}`);
			const payload = JSON.parse(String(init?.body));
			expect(payload.group_id).toBe('234567');
			expect(payload.message[0].data.text).toContain('/战绩');
			sent = true;
			return Response.json({ status: 'ok', retcode: 0, data: { message_id: 902 } });
		}) as typeof fetch;
		await processMessage(env, new QqClient('http://127.0.0.1:3000', token, fetcher), '123456');
		expect(sent).toBe(true);
		const [done] = await env.db.execute(sql`SELECT reply_state FROM qq_inbox WHERE id=${id}`);
		expect(done.reply_state).toBe('done');
		await env.db.execute(sql`UPDATE qq_inbox SET state='pending' WHERE id=${id}`);
		sent = false;
		await processMessage(env, new QqClient('http://127.0.0.1:3000', token, fetcher), '999999');
		expect(sent).toBe(false);
		const [expired] = await env.db.execute(sql`SELECT reply_state FROM qq_inbox WHERE id=${id}`);
		expect(expired.reply_state).toBe('expired');
	});
});
