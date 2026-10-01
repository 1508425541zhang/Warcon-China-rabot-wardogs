import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld, type World } from './world';
import type { Env } from '$lib/server/env';
import { saveQqSettings, qqSettingsView } from '$lib/server/qq/settings';
import { applyQqConfiguration } from '$lib/server/qq/config';
import { OfficialQqClient, officialValidation, officialMessage } from '$lib/server/qq/official';
import { processMessage } from '$lib/server/qq/runtime';
import { POST } from '../routes/api/qq/webhook/+server';
describe.skipIf(!hasTestDb)('official QQ persisted command workflow', () => {
	let env: Env; let world: World;
	const app='1905697626',secret='official-test-secret-1234567890',group='D0804333DCB1FA086972201D6C925069';
	beforeAll(async()=>{env=await testEnv();world=await seedWorld(env);},120000);
	afterAll(async()=>{await env.db.execute(sql`DELETE FROM site_settings WHERE key='qqCommunity'`);applyQqConfiguration(null);});
	const patch = async()=>({revision:(await qqSettingsView(env)).revision,provider:'official',enabled:true,url:'https://api.bot.qq.com',selfId:app,secret,policies:[{serverId:world.server.id,groups:[group],maps:['mapA','mapB']}]});
	test('requires official OpenIDs and encrypts only AppSecret',async()=>{
		const p=await patch(); await expect(saveQqSettings(env,{...p,policies:[{...p.policies[0],groups:['12345678']}]},world.users.site!.id)).rejects.toThrow('group_openid');
		const view=await saveQqSettings(env,p,world.users.site!.id);expect(view.hasSecret).toBe(true);expect(view.hasToken).toBe(false);
		const [row]=await env.db.execute(sql`SELECT value FROM site_settings WHERE key='qqCommunity'`);expect(JSON.stringify(row.value)).not.toContain(secret);
	});
	test('challenge, signed delivery, duplicate rejection and passive reply',async()=>{
		await env.db.execute(sql`UPDATE qq_inbox SET state='done' WHERE state='pending'`);
		const ts=String(Math.floor(Date.now()/1000));
		const payload={op:0,t:'GROUP_AT_MESSAGE_CREATE',d:{id:'ROBOT1.0_integration',author:{member_openid:'A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4'},group_openid:group,content:' /帮助 ',timestamp:new Date().toISOString()}};
		const raw=JSON.stringify(payload),signature=officialValidation(secret,raw,ts).signature;
		const post=(body=raw,sig=signature)=>POST({request:new Request('http://localhost/api/qq/webhook',{method:'POST',headers:{'x-bot-appid':app,'x-signature-timestamp':ts,'x-signature-ed25519':sig},body})} as Parameters<typeof POST>[0]);
		expect((await post(raw,'00'.repeat(64))).status).toBe(401);
		expect((await post(raw+' ')).status).toBe(401);
		expect((await post()).status).toBe(200);expect((await post()).status).toBe(200);
		const id=officialMessage(payload,app)!.id;expect(await env.db.execute(sql`SELECT id FROM qq_inbox WHERE id=${id}`)).toHaveLength(1);
		let sent=false;
		const c=new OfficialQqClient(app,secret,(async(url:unknown,init?:RequestInit)=>{
			if(String(url).includes('getAppAccessToken'))return Response.json({access_token:'mock',expires_in:7200});
			const b=JSON.parse(String(init?.body));expect(b.msg_id).toBe('ROBOT1.0_integration');expect(b.content).toContain('/战绩');sent=true;return Response.json({id:'accepted'});
		}) as typeof fetch);
		await processMessage(env,c,app);expect(sent).toBe(true);
		const [r]=await env.db.execute(sql`SELECT reply_state FROM qq_inbox WHERE id=${id}`);expect(r.reply_state).toBe('done');
		const response=await post(JSON.stringify({op:13,d:{plain_token:'challenge',event_ts:ts}}));expect(response.status).toBe(200);expect(await response.json()).toEqual(officialValidation(secret,'challenge',ts));
	});
});
