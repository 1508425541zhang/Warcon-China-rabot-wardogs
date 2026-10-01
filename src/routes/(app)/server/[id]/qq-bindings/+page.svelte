<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api } from '$lib/api';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	let edit = $state<{action:'add'|'change'|'remove'|'migrate';memberId:string;steamId:string;previousMemberId?:string;expectedSteamId?:string;expectedUserId?:string|null}|null>(null);
	let reason = $state('');
	let busy = $state(false);
	let message = $state('');
	let failure = $state('');
	function open(action:'change'|'remove'|'migrate', row:typeof data.bindings.links[number]) {
		edit={action,memberId:action==='migrate'?'':row.member_id,previousMemberId:action==='migrate'?row.member_id:undefined,steamId:row.steam_id,expectedSteamId:row.steam_id,expectedUserId:row.user_id};reason='';failure='';message='';
	}
	async function save(event:SubmitEvent) {
		event.preventDefault(); if (!edit || busy) return;
		busy=true;failure='';message='';
		try {
			await api('POST', `/api/servers/${encodeURIComponent(data.server.id)}/qq-bindings`, {...edit,reason});
			edit=null;reason='';message='绑定已更新，机器人后续指令会使用新绑定。';await invalidateAll();
		} catch (err) {failure=err instanceof Error?err.message:'操作失败，请重试。';}
		finally {busy=false;}
	}
</script>

<section class="panel p-5">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 class="text-lg font-semibold">官方 QQ 机器人 · 绑定管理</h2>
		{#if data.canManage}<button class="btn" onclick={()=>{edit={action:'add',memberId:'',steamId:''};reason='';failure='';message='';}}>手动新增绑定</button>{/if}
	</div>
	<p class="note mt-3">查看本服务器的绑定。具有玩家管理权限的成员可查看；同时具有自动化管理权限的管理员可新增、更正和解绑。</p>
	<p class="note">官方用户标识由腾讯提供，不能用 QQ 号码代替。请让玩家在群里 @机器人发送 /帮助，再从近期互动用户中选择。游戏昵称取自最近的服务器记录。</p>
	<p class="note">更正、解绑不会转移或清空积分与 VIP；它们仍属于原 SteamID。已经提交的订单保持原订单身份。所有管理操作会记录操作人、原绑定和原因。</p>
	{#if message}<p role="status" class="callout mt-3">{message}</p>{/if}
	{#if failure}<p role="alert" class="callout mt-3 text-danger">{failure}</p>{/if}
	{#if edit && data.canManage}
		<form class="mt-4 rounded border border-white/15 p-4 space-y-3" onsubmit={save}>
			<h3 class="font-semibold">{edit.action==='add'?'手动新增绑定':edit.action==='change'?'更正绑定':edit.action==='migrate'?'迁移旧绑定至官方机器人':'确认解绑'}</h3>
			{#if edit.action==='add' || edit.action==='migrate'}
				<label class="block">近期互动用户
					<select class="input mt-1 w-full" onchange={(event)=>{if(edit)edit.memberId=event.currentTarget.value;}}>
						<option value="">选择用户，或在下方粘贴完整标识</option>
						{#each data.bindings.recent as row}<option value={row.member_id}>{row.member_id} · {row.last_seen}</option>{/each}
					</select>
				</label>
			{/if}
			<label class="block">官方用户标识<input class="input mt-1 w-full font-mono" bind:value={edit.memberId} readonly={edit.action==='change' || edit.action==='remove'} required maxlength="160" placeholder="official:AppID:用户OpenID" /></label>
			{#if edit.action==='migrate'}<p class="note">原用户标识：{edit.previousMemberId}。请核实官方用户身份后选择目标；迁移保留 SteamID、网页验证关系、积分和 VIP。</p>{/if}
			{#if edit.action!=='add' && edit.action!=='migrate'}<p class="note">原 SteamID：{edit.expectedSteamId}。{edit.action==='change'?'更正为其他 SteamID 后，将改为管理员直接绑定。':'解绑后，该用户需要重新绑定才能使用关联玩家的指令。'}</p>{/if}
			{#if edit.action!=='remove'}<label class="block">SteamID64<input class="input mt-1 w-full font-mono" bind:value={edit.steamId} readonly={edit.action==='migrate'} required pattern={'[0-9]{17}'} maxlength="17" placeholder="17 位 SteamID64" /></label>{/if}
			<label class="block">处理原因<textarea class="input mt-1 w-full" bind:value={reason} required maxlength="300" placeholder="例如：玩家绑定错了 SteamID，已核实本人"></textarea></label>
			<div class="flex gap-2"><button class="btn" type="submit" disabled={busy}>{busy?'正在保存…':edit.action==='remove'?'确认解绑':'保存绑定'}</button><button class="btn" type="button" disabled={busy} onclick={()=>{edit=null;failure='';}}>取消</button></div>
		</form>
	{/if}
	<form method="GET" class="mt-5 flex flex-wrap gap-2"><input class="input grow" name="q" value={data.bindings.query} aria-label="搜索绑定" placeholder="搜索 SteamID、游戏昵称或官方用户标识" maxlength="128" /><button class="btn" type="submit">搜索</button><a class="btn" href="?">重置 / 刷新</a></form>
	<p class="note mt-3">共 {data.bindings.total} 条 · 第 {data.bindings.page} 页 · 每页 50 条</p>
	<div class="mt-3 overflow-x-auto"><table class="w-full text-sm"><thead><tr class="border-b border-white/15 text-left"><th class="p-2">游戏昵称 / SteamID64</th><th class="p-2">用户标识 / 来源</th><th class="p-2">绑定方式 / 网页账号</th>{#if data.canManage}<th class="p-2">操作</th>{/if}</tr></thead><tbody>
		{#each data.bindings.links as row (row.member_id)}<tr class="border-b border-white/10"><td class="p-2">{row.name??'暂无游戏记录'}<div class="font-mono text-mist-400">{row.steam_id}</div></td><td class="p-2 max-w-md break-all font-mono">{row.member_id}<div class="mt-1 text-xs text-mist-400">{row.member_id.startsWith('official:')?'腾讯官方 OpenID（不是 QQ 号码）':'旧版绑定：可迁移至官方'}</div>{#if row.previous_member_id}<div class="mt-1 text-xs text-mist-400">原 QQ（迁移记录）：{row.previous_member_id.slice(5)}</div>{/if}</td><td class="p-2">{row.user_id?'网页 Steam 验证':'玩家 / 管理员直接绑定'}{#if row.account_name}<div>{row.account_name}</div>{/if}</td>{#if data.canManage}<td class="p-2"><div class="flex gap-2">{#if row.member_id.startsWith('ob11:')}<button class="btn" onclick={()=>open('migrate',row)}>迁移至官方</button>{/if}<button class="btn" onclick={()=>open('change',row)}>更正</button><button class="btn text-danger" onclick={()=>open('remove',row)}>解绑</button></div></td>{/if}</tr>{/each}
		{#if !data.bindings.links.length}<tr><td colspan={data.canManage?4:3} class="p-5 text-mist-400">{data.bindings.query?'没有匹配的绑定记录。':'本服务器暂时没有绑定记录。'}</td></tr>{/if}
	</tbody></table></div>
	<div class="mt-4 flex gap-3">{#if data.bindings.page>1}<a class="btn" href={`?q=${encodeURIComponent(data.bindings.query)}&page=${data.bindings.page-1}`}>上一页</a>{/if}{#if data.bindings.page*50<data.bindings.total}<a class="btn" href={`?q=${encodeURIComponent(data.bindings.query)}&page=${data.bindings.page+1}`}>下一页</a>{/if}</div>
</section>
