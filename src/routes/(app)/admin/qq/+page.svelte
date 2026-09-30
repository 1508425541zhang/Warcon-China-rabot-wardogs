<script lang="ts">
	import VipSettings from '$lib/components/VipSettings.svelte';
	import { qqProviders } from '$lib/qq-providers';
	import { untrack } from 'svelte';
	import { api, errorMessage, rconGet } from '$lib/api';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	type Config = typeof data.config;
	const edit = (c: Config) => ({
		provider: c.provider,
		enabled: c.enabled,
		url: c.url,
		selfId: c.selfId,
		token: '',
		secret: '',
		clearToken: false,
		clearSecret: false,
		policies: c.policies.map(({ groups, maps, ...p }) => ({
			...p,
			groupsText: groups.join('\n'),
			mapsText: maps.join('\n')
		}))
	});
	let saved = $state(untrack(() => data.config));
	let form = $state(untrack(() => edit(data.config)));
	let provider = $derived(qqProviders[form.provider]);
	let busy = $state(false);
	let dirty = $state(false);
	let message = $state('');
	let failed = $state(false);
	const fields = [
		{ key: 'lowAt', label: '暖服人数上限', unit: '人', min: 1, max: 200 },
		{ key: 'pointsPerMinute', label: '每分钟暖服积分', unit: '积分', min: 1, max: 100 },
		{ key: 'voteCost', label: '每票价格', unit: '积分', min: 1, max: 100000 },
		{ key: 'voteSeconds', label: '投票时长', unit: '秒', min: 30, max: 240 },
		{ key: 'broadcastCost', label: '友方广播价格', unit: '积分', min: 1, max: 100000 },
		{ key: 'reserveCost', label: '预留位价格', unit: '积分', min: 1, max: 100000 },
		{ key: 'reserveHours', label: '预留位有效期', unit: '小时', min: 1, max: 720 }
	] as const;
	const split = (text: string) =>
		text
			.split(/[\n,，]+/)
			.map((s) => s.trim())
			.filter(Boolean);
	function addRule() {
		form.policies.push({
			enabled: true,
			antiCheatNotices: true,
			serverId: data.servers.find((s) => !form.policies.some((p) => p.serverId === s.id))?.id || '',
			groupsText: '',
			mapsText: '',
			lowAt: 20,
			pointsPerMinute: 1,
			voteCost: 10,
			voteSeconds: 120,
			broadcastCost: 20,
			reserveCost: 120,
			reserveHours: 24
		});
		dirty = true;
	}
	let catalogs = $state<Record<string, { id: string; display: string }[]>>({});
	async function readMaps(serverId: string) {
		busy = true;
		message = '';
		failed = false;
		try {
			catalogs[serverId] = (
				await rconGet<{ maps: { id: string; display: string }[] }>(serverId, 'maps')
			).maps;
		} catch (e) {
			failed = true;
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	function toggleMap(index: number, id: string) {
		const p = form.policies[index];
		const ids = split(p.mapsText);
		p.mapsText = (ids.includes(id) ? ids.filter((m) => m !== id) : [...ids, id]).join('\n');
		dirty = true;
	}
	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		message = '';
		failed = false;
		try {
			const patch = {
				...$state.snapshot(form),
				revision: saved.revision,
				policies: form.policies.map(({ groupsText, mapsText, ...p }) => ({
					...p,
					groups: split(groupsText),
					maps: split(mapsText)
				}))
			};
			const result = await api<{ config: Config }>('PUT', '/api/admin/qq', patch);
			saved = result.config;
			form = edit(result.config);
			dirty = false;
			message = '配置已保存。机器人将在下一轮处理时读取，暖服工作进程通常在十秒内更新。';
		} catch (e) {
			failed = true;
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	async function reload() {
		busy = true;
		message = '';
		failed = false;
		try {
			const result = await api<{ config: Config }>('GET', '/api/admin/qq');
			saved = result.config;
			form = edit(result.config);
			dirty = false;
		} catch (e) {
			failed = true;
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
	async function test() {
		busy = true;
		message = '';
		failed = false;
		try {
			const result = await api<{ message: string }>('POST', '/api/admin/qq', {});
			message = result.message;
		} catch (e) {
			failed = true;
			message = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>QQ 机器人 · 站点管理 · {data.appName}</title></svelte:head>
<div class="mb-5 flex flex-wrap items-start justify-between gap-3">
	<div>
		<h2 class="text-xl font-semibold">QQ 机器人</h2>
		<p class="mt-1 text-sm text-mist-400">连接 NapCat 或 LLBot，管理群查询、暖服积分和兑换规则。</p>
	</div>
	<span class="rounded-full border border-white/15 px-3 py-1 text-sm"
		>已保存状态：{saved.enabled ? '启用' : '停用'}</span
	>
</div>
{#if message}<div
		role={failed ? 'alert' : 'status'}
		class="mb-4 rounded border p-3 text-sm {failed
			? 'border-red-400/60 text-red-300'
			: 'border-emerald-400/40 text-emerald-200'}"
	>
		{message}
	</div>{/if}
<form onsubmit={save} oninput={() => (dirty = true)} onchange={() => (dirty = true)}>
	<fieldset disabled={busy} class="space-y-5">
		<section class="space-y-4 panel">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<h3 class="font-semibold">机器人连接</h3>
				<label class="flex items-center gap-2"
					><input type="checkbox" bind:checked={form.enabled} />启用 QQ 社区服务</label
				>
			</div>
			<p class="text-sm text-mist-400">
				停用后不再接收新指令或新增暖服积分，历史绑定和积分保留。已经开始的操作可能完成。
			</p>
			<label class="block space-y-1">
				<span class="label-sm">机器人架构</span>
				<select class="input w-full" bind:value={form.provider}>
					<option value="napcat">NapCat · OneBot 11</option>
					<option value="llbot">LLBot（LuckyLilliaBot）· OneBot 11</option>
				</select>
				<span class="block text-xs text-mist-500"
					>切换架构后请填写对应连接并保存。相同 QQ 号的绑定和积分继续使用。</span
				>
			</label>
			<div class="grid gap-4 md:grid-cols-2">
				<label class="space-y-1"
					><span class="label-sm">{provider.name} HTTP 接口地址</span><input
						class="input w-full"
						type="url"
						bind:value={form.url}
						placeholder={provider.placeholder}
						maxlength="2000"
						required={form.enabled}
					/><span class="block text-xs text-mist-500">同机使用回环地址，远程连接使用 HTTPS。</span
					></label
				>
				<label class="space-y-1"
					><span class="label-sm">机器人 QQ 号</span><input
						class="input w-full"
						bind:value={form.selfId}
						inputmode="numeric"
						pattern={'[1-9][0-9]{4,15}'}
						placeholder="登录机器人的 QQ 号"
						required={form.enabled}
					/></label
				>
				<div class="space-y-2">
					<label class="block space-y-1"
						><span class="label-sm"
							>API 访问密钥 {saved.hasToken ? '（已设置）' : '（未设置）'}</span
						><input
							class="input w-full"
							type="password"
							bind:value={form.token}
							autocomplete="new-password"
							placeholder={saved.hasToken ? '留空保留原密钥' : '与机器人 HTTP 服务端 token 一致'}
							minlength="16"
							maxlength="512"
						/></label
					><label class="flex items-center gap-2 text-xs text-mist-400"
						><input type="checkbox" bind:checked={form.clearToken} />清除已保存的 API 密钥</label
					>
				</div>
				<div class="space-y-2">
					<label class="block space-y-1"
						><span class="label-sm"
							>事件签名密钥 {saved.hasSecret ? '（已设置）' : '（未设置）'}</span
						><input
							class="input w-full"
							type="password"
							bind:value={form.secret}
							autocomplete="new-password"
							placeholder={saved.hasSecret ? '留空保留原密钥' : '与机器人 HTTP 上报 token 一致'}
							minlength="16"
							maxlength="512"
						/></label
					><label class="flex items-center gap-2 text-xs text-mist-400"
						><input type="checkbox" bind:checked={form.clearSecret} />清除已保存的事件密钥</label
					>
				</div>
			</div>
			<div class="rounded border border-white/10 p-3 text-sm">
				<p class="mb-1 font-medium">{provider.name} 事件上报地址</p>
				<code class="break-all select-all">{data.callback}</code>
				<p class="mt-2 text-mist-400">
					{provider.setup} QQ 登录与扫码在机器人中完成。
				</p>
				<a
					class="mt-2 inline-block underline"
					href={provider.guide}
					target="_blank"
					rel="noreferrer">{provider.name} 官方配置文档</a
				>
			</div>
			<button class="btn" type="button" onclick={test} disabled={busy || dirty}
				>检测已保存的连接</button
			><span class="ml-2 text-xs text-mist-500">检测登录账号，不向群发送消息。修改后请先保存。</span
			>
		</section>
		<section class="space-y-4">
			<div class="flex items-center justify-between gap-3">
				<h3 class="font-semibold">服务器与积分规则</h3>
				<button
					type="button"
					class="btn"
					onclick={addRule}
					disabled={form.policies.length >= data.servers.length}>添加服务器规则</button
				>
			</div>
			{#if !data.servers.length}<p class="panel text-sm text-mist-400">
					请先在组织中添加游戏服务器，再配置群规则。
				</p>{:else if !form.policies.length}<p class="panel text-sm text-mist-400">
					尚未配置服务器。添加规则后，授权群成员才能查询对应服务器及使用积分服务。
				</p>{/if}
			{#each form.policies as p, index}
				<div class="space-y-4 panel">
					<div class="flex flex-wrap items-center justify-between gap-3">
						<h4 class="font-medium">服务器规则 {index + 1}</h4>
						<div class="flex gap-4">
							<label class="flex items-center gap-2 text-sm"
								><input type="checkbox" bind:checked={p.enabled} />启用本服</label
							><button
								type="button"
								class="text-sm text-red-300 underline"
								onclick={() => {
									form.policies.splice(index, 1);
									dirty = true;
								}}>移除规则</button
							>
						</div>
					</div>
					<label class="block space-y-1"
						><span class="label-sm">反作弊踢出群通知</span><input
							type="checkbox"
							bind:checked={p.antiCheatNotices}
						/>
					</label>
					<p class="text-sm text-mist-400">
						确认反作弊踢出成功后，向本服授权群发送理由、实际处罚档位和
						AI、规则或管理员来源。普通踢出不发送。
					</p>
					<label class="block space-y-1"
						><span class="label-sm">游戏服务器</span><select
							class="input w-full"
							bind:value={p.serverId}
							required
							><option value="" disabled>选择服务器</option>{#each data.servers as server}<option
									value={server.id}>{server.orgName} / {server.name}</option
								>{/each}</select
						></label
					>
					<div class="grid gap-4 md:grid-cols-2">
						<label class="space-y-1"
							><span class="label-sm">授权 QQ 群号</span><textarea
								class="min-h-28 input w-full"
								bind:value={p.groupsText}
								required
								placeholder="每行一个数字群号，也可用逗号分隔"></textarea><span
								class="block text-xs text-mist-500"
								>每个群只能关联一台服务器；群内可查询本服战绩及在线名单。</span
							></label
						>
						<label class="space-y-1"
							><span class="label-sm">投票候选地图 ID</span><textarea
								class="min-h-28 input w-full"
								bind:value={p.mapsText}
								required
								placeholder="每行一个真实地图 ID，填写 2–10 个"></textarea><span
								class="block text-xs text-mist-500">按此顺序显示；平票时顺序靠前的地图胜出。</span
							></label
						>
					</div>
					<div class="space-y-2">
						<button
							type="button"
							class="btn"
							onclick={() => readMaps(p.serverId)}
							disabled={!p.serverId}>读取游戏地图目录</button
						>
						{#if catalogs[p.serverId]}
							<div class="flex flex-wrap gap-3">
								{#each catalogs[p.serverId] as map}<label
										class="flex items-center gap-2 rounded border border-white/10 px-3 py-2 text-sm"
										><input
											type="checkbox"
											checked={split(p.mapsText).includes(map.id)}
											onchange={() => toggleMap(index, map.id)}
										/>{map.display} <span class="text-xs text-mist-500">{map.id}</span></label
									>{/each}
							</div>
						{/if}
					</div>
					<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
						{#each fields as field}<label class="space-y-1"
								><span class="label-sm">{field.label}（{field.unit}）</span><input
									class="input w-full"
									type="number"
									bind:value={p[field.key]}
									min={field.min}
									max={field.max}
									step="1"
									required
								/><span class="block text-xs text-mist-500">{field.min}–{field.max}</span></label
							>{/each}
					</div>
				</div>
			{/each}
		</section>
		<div class="flex flex-wrap items-center gap-3 panel">
			<button type="submit" class="btn btn-primary">{busy ? '处理中…' : '保存配置'}</button><button
				type="button"
				class="btn"
				onclick={reload}>重新加载并放弃修改</button
			><span class="text-sm text-mist-400"
				>{dirty
					? '有未保存的修改'
					: saved.source === 'database'
						? '当前使用网页保存的配置'
						: '尚未保存，当前使用环境默认配置'}</span
			>
		</div>
	</fieldset>
</form>
<section class="mt-5 panel text-sm text-mist-400">
	<h3 class="mb-2 font-medium text-mist-200">群内使用</h3>
	<p>
		/帮助、/服务器、/在线、/地图、/战绩、/总结；完成 <a href="/qq-link" class="underline"
			>QQ 与 Steam 绑定</a
		> 后使用 /举报、/积分、/流水、/投票、/友方广播、/优先队列、/订单。
	</p>
	<p class="mt-2">友方广播按当前阵营逐人发送游戏消息；优先队列兑换的是游戏支持的预留位。</p>
</section>

<VipSettings initial={data.vips} servers={data.servers} />
