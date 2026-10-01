import { beforeAll, describe, expect, test, spyOn } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld, type World } from './world';
import { callApi, stubGateway } from './call';
import { gateway } from '$lib/server/gateway';
import { getServer } from '$lib/server/access';
import type { Env } from '$lib/server/env';
import {
	saveFactionQuota,
	quotaConfig,
	factionQuotaView,
	runFactionQuota
} from '$lib/server/faction-quota';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { POST } from '../routes/api/servers/[id]/faction-quota/+server';
import type { WardogsClient } from '$lib/server/rcon';
import type { Status, Player } from '$lib/types';

describe.skipIf(!hasTestDb)('50V50 persisted automation', () => {
	let env: Env, world: World;
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
		stubGateway();
	}, 120000);
	test('unauthorized accounts cannot save or affect the game', async () => {
		const input = {
			method: 'POST',
			params: { id: world.server.id },
			body: { revision: 'empty', enabled: false, limits: { blue: 50, red: 50, green: 0 } }
		};
		expect((await callApi(POST, null, input)).status).toBe(401);
		expect((await callApi(POST, world.users.viewer, input)).status).toBe(403);
		expect((await callApi(POST, world.users.operator, input)).status).toBe(403);
	});
	test('save disabled makes no game calls; enable edits one native switch and waits next match; disable restores it', async () => {
		const server = (await getServer(env, world.server.id))!;
		const body = {
			revision: 'empty',
			enabled: false,
			limits: { blue: 50, red: 50, green: 0 },
			graceSeconds: 60
		};
		const disabled = await saveFactionQuota(env, server, world.users.admin!.id, body);
		expect(disabled.enabled).toBe(false);
		expect((await quotaConfig(env, server.id)).revision).toBe(disabled.revision);
		const [match] = await env.db.execute<{ id: number }>(
			sql`INSERT INTO matches(server_id,started_at,map) VALUES(${server.id},now()-interval '10 minutes','Bakurani') RETURNING id`
		);
		let text =
			'[/Script/WDGame.WDGameStateSession]\nbLockOverpopulatedTeamsConfig=True\nOverpopulatedTeamThresholdConfig=2\n[Other]\nKeep=unchanged\n';
		const mock = spyOn(gateway(), 'run').mockImplementation(
			async (_env, _server, action, params) => {
				if (action === 'capabilities')
					return { features: { changeTeam: true, configDocument: true } };
				if (action === 'config') return { revision: 'etag-1', writable: true, text, sections: [] };
				if (action === 'configApply') {
					expect(params.revision).toBe('etag-1');
					expect(String(params.text)).toContain('Keep=unchanged');
					text = String(params.text);
					return { ok: true };
				}
				throw new Error('Unexpected action');
			}
		);
		try {
			const enabled = await saveFactionQuota(env, server, world.users.admin!.id, {
				...body,
				revision: disabled.revision,
				enabled: true
			});
			expect(text).toContain('bLockOverpopulatedTeamsConfig=false');
			expect(enabled.waitMatchId).toBe(Number(match.id));
			expect(enabled.originalJoinLock).toBe('True');
			expect((await quotaConfig(env, server.id)).enabled).toBe(true);
			await expect(
				saveFactionQuota(env, server, world.users.admin!.id, {
					...body,
					revision: disabled.revision
				})
			).rejects.toThrow('设置已变化');
			await saveFactionQuota(env, server, world.users.admin!.id, {
				...body,
				revision: enabled.revision
			});
			expect(text).toContain('bLockOverpopulatedTeamsConfig=True');
		} finally {
			mock.mockRestore();
		}
	});
	test('fresh snapshots confirm one move before another; stopped configuration never moves players', async () => {
		const server = (await getServer(env, world.server.id))!;
		const old = await quotaConfig(env, server.id);
		await env.db.execute(
			sql`UPDATE site_settings SET value=${JSON.stringify({ ...old, enabled: true, waitMatchId: null })}::text::jsonb WHERE key=${'factionQuota:' + server.id}`
		);
		const scores = [
			{ name: 'Lonestar', colorHex: '#4CB1EF', score: 0 },
			{ name: 'Valkyra', colorHex: '#FA503E', score: 0 },
			{ name: 'Manticore', colorHex: '#1DD65C', score: 0 }
		];
		const status = { map: 'Bakurani', scores, scoreCap: null } as Status;
		const players = [1, 2].map((n) => ({
			steamId: String(76561198000000100n + BigInt(n)),
			faction: 'Manticore',
			kills: 0
		})) as Player[];
		const calls: string[] = [];
		const client = {
			raw: async () => ({
				status: 200,
				headers: {},
				text: JSON.stringify({
					revision: 'etag',
					sections: [],
					text: '[/Script/WDGame.WDGameStateSession]\nbLockOverpopulatedTeamsConfig=false'
				})
			}),
			json: async (method: string, path: string) => {
				calls.push(method + ' ' + path);
				return {};
			}
		} as unknown as WardogsClient;
		expect(await acquireOrRenew(env, 'quota-test')).toBe(true);
		const input = () => ({
			players,
			status,
			statusAt: Date.now(),
			now: new Date(),
			trusted: true,
			boundary: false,
			startupAt: Date.now() - 600000
		});
		try {
			await runFactionQuota(env, server, client, { ...input(), trusted: false });
			await runFactionQuota(env, server, client, { ...input(), statusAt: Date.now() - 31000 });
			expect(calls).toHaveLength(0);
			await runFactionQuota(env, server, client, input());
			expect(calls.filter((c) => c.startsWith('PATCH'))).toHaveLength(1);
			await runFactionQuota(env, server, client, input());
			expect(calls.filter((c) => c.startsWith('PATCH'))).toHaveLength(1);
			const view = await factionQuotaView(env, server.id);
			players[0].faction = view.runtime.events[0].to;
			await runFactionQuota(env, server, client, input());
			expect(calls.filter((c) => c.startsWith('PATCH'))).toHaveLength(2);
			await env.db.execute(
				sql`UPDATE site_settings SET value=jsonb_set(value,'{enabled}','false') WHERE key=${'factionQuota:' + server.id}`
			);
			await runFactionQuota(env, server, client, input());
			expect(calls.filter((c) => c.startsWith('PATCH'))).toHaveLength(2);
		} finally {
			await releaseOwnership(env);
		}
	});
	test('failed configuration stays disabled and unsupported official interfaces never activate the rule', async () => {
		const server = (await getServer(env, world.server.id))!;
		const config = await quotaConfig(env, server.id);
		const mock = spyOn(gateway(), 'run').mockImplementation(async () => ({
			features: { changeTeam: false, configDocument: true }
		}));
		try {
			await expect(
				saveFactionQuota(env, server, world.users.admin!.id, { ...config, enabled: true })
			).rejects.toThrow('不支持官方管理员调队');
			const actual = await quotaConfig(env, server.id);
			expect(actual.enabled).toBe(false);
			expect(actual.preparing).toBe(false);
		} finally {
			mock.mockRestore();
		}
	});
});
