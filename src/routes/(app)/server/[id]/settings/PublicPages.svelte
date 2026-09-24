<script lang="ts">
	// The server's public pages (a section of the Settings tab): a live status page and a
	// leaderboard with career pages, each open to anyone with the address once switched on here
	// (the site owner can close them for the organisation). Org owners only, like the server
	// dialog that carries the same switches.
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import { featureState, FEATURE_LABELS, PUBLIC_FEATURES, type PublicFeature } from '$lib/features';
	import type { ServerInfo } from '$lib/types';

	let { data }: { data: { server: ServerInfo; origin: string } } = $props();
	let busy = $state(false);
	let orgPage = $derived(`/orgs/${encodeURIComponent(data.server.orgId)}`);

	const KEY = { status: 'publicStatus', leaderboards: 'publicLeaderboards' } as const;
	const PATH = { status: '', leaderboards: '/leaderboard' } as const;
	const ABOUT: Record<PublicFeature, string> = {
		status: '公开地图、模式、比分、人数、加入代码及在线玩家的击杀和死亡数据，每 20 秒刷新。',
		leaderboards:
			'显示与“排行榜”选项卡相同的榜单，并为每位玩家提供生涯页面；范围可选本服或组织的公开服务器。'
	};
	const address = (feature: PublicFeature) =>
		`${data.origin}/s/${encodeURIComponent(data.server.id)}${PATH[feature]}`;

	async function patch(body: Record<string, boolean>, done: string) {
		busy = true;
		try {
			await api('PATCH', `/api/servers/${encodeURIComponent(data.server.id)}`, body);
			toast(done, 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	const setPublic = (feature: PublicFeature, on: boolean) =>
		patch({ [KEY[feature]]: on }, `${FEATURE_LABELS[feature]} is ${on ? 'on' : 'off'}.`);
	const setKills = (on: boolean) =>
		patch({ publicKills: on }, on ? '状态页已显示击杀事件。' : '状态页已隐藏击杀事件。');
	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast('地址已复制。', 'ok');
		} catch {
			window.prompt('请复制地址：', text);
		}
	}
</script>

<div class="panel">
	<span class="label-sm">公开页面</span>
	<p class="mb-4 text-[13px] text-mist-400">
		无需登录即可打开的页面。页面只显示游戏昵称，不显示
		SteamID、延迟、现金或管理面板信息；服务器无法连接时也只显示连接失败。每个页面在 <code
			class="chip">/api/public/servers/{data.server.id}</code
		>.
	</p>
	{#if !data.server.manager}
		<p class="note">只有以下组织的所有者： {data.server.orgName} 可以启用这些选项。</p>
	{:else}
		{#each PUBLIC_FEATURES as feature (feature)}
			{@const st = featureState(data.server, data.server, feature)}
			<div class="border-b border-white/[0.06] py-4 last:border-0">
				<label class="flex items-center gap-2 text-[14px] {st.allowed ? '' : 'opacity-50'}">
					<input
						type="checkbox"
						checked={st.wanted}
						disabled={busy || !st.allowed}
						onchange={(e) => setPublic(feature, e.currentTarget.checked)}
					/>
					<span class="font-medium">{FEATURE_LABELS[feature]}</span>
					{#if st.on}<Badge tone="ok">公开</Badge>{:else if st.reason}<Badge tone="err"
							>已关闭</Badge
						>{:else}<Badge>关闭</Badge>{/if}
				</label>
				<p class="mt-1 pl-6 text-[13px] text-mist-400">{ABOUT[feature]}</p>
				<div class="mt-2 pl-6">
					{#if st.reason}
						<span class="text-[12.5px] text-mist-400">{st.reason}</span>
					{:else}
						<div class="join w-full sm:w-auto">
							<a
								href={address(feature)}
								target="_blank"
								rel="noopener noreferrer"
								class="flex input items-center font-mono text-[12.5px] break-all {st.on
									? ''
									: 'text-mist-600'}">{address(feature)}</a
							>
							<button type="button" class="btn btn-sm h-auto" onclick={() => copy(address(feature))}
								>复制</button
							>
						</div>
						{#if !st.on}<p class="note">启用前访问会返回 404。</p>{/if}
					{/if}
					{#if feature === 'status' && !st.reason}
						<label class="mt-3 flex items-center gap-2 text-[13.5px]">
							<input
								type="checkbox"
								checked={data.server.publicKills}
								disabled={busy}
								onchange={(e) => setKills(e.currentTarget.checked)}
							/>
							<span>在页面显示击杀事件</span>
						</label>
						<p class="mt-1 pl-6 text-[12.5px] text-mist-400">
							显示最近 20 条击杀及武器和距离，只显示昵称。需先在“击杀”标签页启用击杀事件。
						</p>
					{/if}
				</div>
			</div>
		{/each}
		<p class="note">
			Discord 状态卡片可链接这些页面；请在上方频道设置中选择链接。所有公开页面的 Discord 邀请按钮在
			<a href={orgPage} class="text-accent hover:underline">组织页面</a>.
		</p>
	{/if}
</div>
