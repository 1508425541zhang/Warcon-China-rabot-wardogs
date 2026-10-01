import { describe, expect, test } from 'bun:test';
import { officialMessage, OfficialQqClient, officialValidation, verifyOfficial, validQqIdentity } from './official';
const secret = 'naOC0ocQE3shWLAfffVLB1rhYPG7';
const group = 'B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4E5';
const member = 'A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4';
const event = () => ({op:0,t:'GROUP_AT_MESSAGE_CREATE',d:{id:'ROBOT1.0_message==',group_openid:group,author:{member_openid:member},content:' /积分 ',timestamp:new Date().toISOString()}});
describe('Tencent official QQ protocol', () => {
	test('verifies raw bytes and rejects tampering and stale timestamps', () => {
		const body = '{ "op": 0,"d": {}, "t": "GATEWAY_EVENT_NAME"}';
		const signature=officialValidation(secret,body,'1725442341').signature;
		const h=new Headers({'x-signature-timestamp':'1725442341','x-signature-ed25519':signature});
		expect(verifyOfficial(secret,h,Buffer.from(body),1725442341000)).toBe(true);
		expect(verifyOfficial(secret,h,Buffer.from(body+' '),1725442341000)).toBe(false);
		expect(verifyOfficial(secret,h,Buffer.from(body),1725442941000)).toBe(false);
	});
	test('matches official challenge signature exactly', () => {
		expect(officialValidation('DG5g3B4j9X2KOErG','Arq0D5A61EgUu4OxUvOp','1725442341').signature).toBe('87befc99c42c651b3aac0278e71ada338433ae26fcb24307bdc5ad38c1adc2d01bcfcadc0842edac85e85205028a1132afe09280305f13aa6909ffc2d652c706');
	});
	test('isolates app OpenID identities and preserves reply message ID', () => {
		const m=officialMessage(event(),'1905697626')!;
		expect(m.content).toBe('/积分'); expect(m.groupId).toBe(group);
		expect(m.memberId).toBe('official:1905697626:'+member);
		expect(validQqIdentity(m.memberId)).toBe(true);
		expect(validQqIdentity('official:1905697626:123456')).toBe(false);
		expect(Buffer.from(m.id.split(':')[3],'base64url').toString()).toBe('ROBOT1.0_message==');
		const old=event(); old.d.timestamp=new Date(Date.now()-301000).toISOString(); expect(officialMessage(old,'1905697626')).toBeNull();
		const text=event(); text.d.content='普通消息'; expect(officialMessage(text,'1905697626')).toBeNull();
	});
	test('uses official token cache, auth and passive/active group bodies', async () => {
		const calls: {url:string;init?:RequestInit}[]=[];
		const fetcher=(async (url: string|URL|Request,init?:RequestInit) => { calls.push({url:String(url),init}); return Response.json(String(url).includes('getAppAccessToken') ? {access_token:'test-access',expires_in:'7200'} : {id:'sent-id'}); }) as typeof fetch;
		const c=new OfficialQqClient('1905697626','test-secret-123456',fetcher);
		await Promise.all([c.token(),c.token()]);
		expect(calls.length).toBe(1);
		await c.reply(group,officialMessage(event(),'1905697626')!.id,'战绩');
		expect(calls[1].url).toBe('https://api.bot.qq.com/v2/groups/'+group+'/messages');
		expect((calls[1].init?.headers as Record<string,string>).authorization).toBe('QQBot test-access');
		expect(JSON.parse(String(calls[1].init?.body))).toEqual({content:'战绩',msg_type:0,msg_id:'ROBOT1.0_message==',msg_seq:1});
		await c.reply(group,'integrity:42','已踢出');
		expect(JSON.parse(String(calls[2].init?.body))).toEqual({content:'已踢出',msg_type:0});
	});
	test('rejects HTTP 200 business failure and missing delivery confirmation', async () => {
		const failed=new OfficialQqClient('1905697626','test',(async()=>Response.json({code:100016})) as unknown as typeof fetch);
		await expect(failed.token()).rejects.toThrow('credentials rejected');
		const missing=new OfficialQqClient('1905697626','test',(async(url)=>Response.json(String(url).includes('getAppAccessToken') ? {access_token:'x',expires_in:7200}:{})) as typeof fetch);
		await expect(missing.reply(group,'integrity:1','x')).rejects.toThrow('confirm delivery');
	});
});
