import { beforeAll,afterAll,describe,test,expect } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv,hasTestDb } from './db';
import { seedWorld,type World } from './world';
import { callApi,callLoad } from './call';
import type { Env } from '$lib/server/env';
import { getServer } from '$lib/server/access';
import { applyQqConfiguration,parsePolicies } from '$lib/server/qq/config';
import { bindingsView,manageBinding } from '$lib/server/qq/bindings-admin';
import { linkedAccount,createLinkCode,bindAccount,unbindQq,bindByPlayer } from '$lib/server/qq/identity';
import { GET,POST } from '../routes/api/servers/[id]/qq-bindings/+server';
import { load } from '../routes/(app)/server/[id]/qq-bindings/+page.server';

describe.skipIf(!hasTestDb)('official QQ binding administration',()=>{
	let env:Env;let w:World;
	const member='official:1905697626:AABBCCDDEEFF00112233445566778899';
	const member2='official:1905697626:BBCCDDEEFF0011223344556677889900';
	const steam='76561198000000001', other='76561198000000002';
	const body={action:'add',memberId:member,steamId:steam,reason:'人工核实补绑'};
	beforeAll(async()=>{
		env=await testEnv();w=await seedWorld(env);
		applyQqConfiguration({provider:'official',enabled:true,url:'https://api.bot.qq.com',selfId:'1905697626',token:'',secret:'test-secret',policies:parsePolicies(JSON.stringify([{serverId:w.server.id,groups:['AABBCCDDEEFF00112233445566778899'],maps:['a','b']}]))});
	},120000);
	afterAll(()=>applyQqConfiguration(null));
	test('server permissions apply to API and page data; operators view but cannot edit',async()=>{
		for(const name of ['anon','viewer','outsider','elsewhere','keyAll'] as const){
			expect((await callApi(GET,w.users[name],{params:{id:w.server.id}})).status).not.toBe(200);
			expect((await callLoad(load,w.users[name],{params:{id:w.server.id}})).status).not.toBe(200);
		}
		expect((await callApi(GET,w.users.operator,{params:{id:w.server.id}})).status).toBe(200);
		for(const name of ['operator','viewer','outsider','keyAll'] as const)expect((await callApi(POST,w.users[name],{method:'POST',params:{id:w.server.id},body})).status).not.toBe(200);
	});
	test('manual add is used by bot, conflicts never overwrite, and recent identities are scoped',async()=>{
		expect((await callApi(POST,w.users.admin,{method:'POST',params:{id:w.server.id},body})).status).toBe(200);
		expect((await linkedAccount(env,w.server.id,member)).steamId).toBe(steam);
		const server=(await getServer(env,w.server.id))!;
		await expect(manageBinding(env,server,w.users.admin!,{...body,memberId:member2})).rejects.toThrow('已绑定其他');
		await expect(manageBinding(env,server,w.users.admin!,{...body,memberId:'ob11:12345',steamId:other})).rejects.toThrow('当前官方');
		await env.db.execute(sql`INSERT INTO qq_inbox(id,server_id,group_id,member_id,content,state) VALUES('bindings-local',${w.server.id},'group',${member},'/帮助','done'),('bindings-other',${w.otherServer.id},'group',${member2},'/帮助','done')`);
		const view=await bindingsView(env,w.server.id,steam);
		expect(view.total).toBe(1);expect(view.recent.map(r=>r.member_id)).toEqual([member]);
	});
	test('correction clears old link codes and stale administrator actions cannot remove new binding',async()=>{
		const server=(await getServer(env,w.server.id))!;
		const code=await createLinkCode(env,w.server.id,member);
		await env.db.execute(sql`INSERT INTO qq_wallets(server_id,steam_id,balance) VALUES(${w.server.id},${steam},42)`);
		await manageBinding(env,server,w.users.admin!,{action:'change',memberId:member,steamId:other,expectedSteamId:steam,expectedUserId:null,reason:'纠正错误 SteamID'});
		expect((await linkedAccount(env,w.server.id,member)).steamId).toBe(other);
		await expect(bindAccount(env,code,w.users.admin!)).rejects.toThrow('已过期');
		await expect(manageBinding(env,server,w.users.admin!,{action:'remove',memberId:member,expectedSteamId:steam,expectedUserId:null,reason:'旧页面请求'})).rejects.toThrow('已发生变化');
		await manageBinding(env,server,w.users.admin!,{action:'remove',memberId:member,expectedSteamId:other,expectedUserId:null,reason:'重新绑定前解绑'});
		await expect(linkedAccount(env,w.server.id,member)).rejects.toThrow('尚未绑定');
		const [wallet]=await env.db.execute(sql`SELECT balance FROM qq_wallets WHERE server_id=${w.server.id} AND steam_id=${steam}`);expect(Number(wallet.balance)).toBe(42);
		const audit=await env.db.execute(sql`SELECT action,detail FROM audit_log WHERE server_id=${w.server.id} AND action LIKE 'qq.binding.%' ORDER BY id`);
		expect(audit.map(a=>a.action)).toEqual(['qq.binding.add','qq.binding.change','qq.binding.remove']);expect(JSON.stringify(audit)).toContain('纠正错误');
	});
	test('concurrent claims for same SteamID produce one winner; player self-unbind shares lock',async()=>{
		const server=(await getServer(env,w.server.id))!;
		const outcomes=await Promise.allSettled([member,member2].map(memberId=>manageBinding(env,server,w.users.admin!,{...body,memberId})));
		expect(outcomes.filter(r=>r.status==='fulfilled')).toHaveLength(1);
		const view=await bindingsView(env,w.server.id);expect(view.links).toHaveLength(1);
		await unbindQq(env,w.server.id,view.links[0].member_id);
		expect((await bindingsView(env,w.server.id)).total).toBe(0);
	});
	test('verified official rebind migrates legacy identity with wallet intact; official accounts cannot be overwritten',async()=>{
		await env.db.execute(sql`INSERT INTO qq_links(server_id,member_id,steam_id,user_id) VALUES(${w.server.id},'ob11:12345678',${steam},NULL)`);
		const code=await createLinkCode(env,w.server.id,'ob11:12345678');
		const roster=[{steamId:steam,name:'Verified name',faction:'blue'}];
		await expect(bindByPlayer(env,w.server.id,member,steam,'wrong name',roster)).rejects.toThrow('不匹配');
		expect((await bindingsView(env,w.server.id)).links[0].member_id).toBe('ob11:12345678');
		expect(await bindByPlayer(env,w.server.id,member,steam,'Verified name',roster)).toMatchObject({migrated:true,steamId:steam});
		expect((await linkedAccount(env,w.server.id,member)).steamId).toBe(steam);
		await expect(linkedAccount(env,w.server.id,'ob11:12345678')).rejects.toThrow('尚未绑定');
		expect((await bindingsView(env,w.server.id)).links[0].previous_member_id).toBe('ob11:12345678');
		await expect(bindAccount(env,code,w.users.admin!)).rejects.toThrow('已过期');
		await expect(bindByPlayer(env,w.server.id,member2,steam,'Verified name',roster)).rejects.toThrow('其他 QQ');
		expect((await bindByPlayer(env,w.server.id,member,steam,'Verified name',roster)).already).toBe(true);
		const [wallet]=await env.db.execute(sql`SELECT balance FROM qq_wallets WHERE server_id=${w.server.id} AND steam_id=${steam}`);expect(Number(wallet.balance)).toBe(42);
		await unbindQq(env,w.server.id,member);
	});
	test('administrator migration preserves verified website account and requires the original SteamID',async()=>{
		const server=(await getServer(env,w.server.id))!;
		await env.db.execute(sql`INSERT INTO account(id,account_id,provider_id,user_id,created_at,updated_at) VALUES('migration-account',${other},'steam',${w.users.admin!.id},now(),now())`);
		await env.db.execute(sql`INSERT INTO qq_links(server_id,member_id,steam_id,user_id) VALUES(${w.server.id},'ob11:87654321',${other},${w.users.admin!.id})`);
		const migration={action:'migrate',previousMemberId:'ob11:87654321',memberId:member,steamId:other,expectedSteamId:other,expectedUserId:w.users.admin!.id,reason:'核验后迁移官方标识'};
		await expect(manageBinding(env,server,w.users.admin!,{...migration,steamId:steam})).rejects.toThrow('必须保持一致');
		await expect(manageBinding(env,server,w.users.admin!,{...migration,expectedUserId:null})).rejects.toThrow('已发生变化');
		await manageBinding(env,server,w.users.admin!,migration);
		const linked=await linkedAccount(env,w.server.id,member);expect(linked.steamId).toBe(other);expect(linked.actor.id).toBe(w.users.admin!.id);
		await expect(manageBinding(env,server,w.users.admin!,migration)).rejects.toThrow('已发生变化');
		const [audit]=await env.db.execute(sql`SELECT detail FROM audit_log WHERE server_id=${w.server.id} AND action='qq.binding.migrate' AND actor_id=${w.users.admin!.id}`);expect(JSON.stringify(audit.detail)).toContain('ob11:87654321');
	});
});
