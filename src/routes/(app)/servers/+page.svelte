<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtNum } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import GrantList from '$lib/components/GrantList.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { OrgMemberView, RoleView, ServerInfo, Status, Features } from '$lib/types';
	import { FEATURE_LABELS, PUBLIC_FEATURES, featureState, NO_ALLOWANCES } from '$lib/features';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	type TestOk = {
		ok: true;
		status: Status;
		capabilities: {
			routes: string[];
			features: Features;
			/** the capabilities document as the server sent it (version, build, limits…) */
			raw?: Record<string, unknown>;
		} | null;
		durationMs: number;
		/** the join code, on builds that serve GET /v1/server-id */
		serverId?: string;
	};
	type Dialog =
		| {
				kind: 'edit';
				server: ServerInfo | null;
				orgId: string;
				name: string;
				host: string;
				port: string;
				scheme: 'http' | 'https';
				password: string;
				notes: string;
				sortOrder: string;
				publicStatus: boolean;
				publicLeaderboards: boolean;
		  }
		| { kind: 'test'; server: ServerInfo; result: TestOk }
		| {
				kind: 'access';
				server: ServerInfo;
				members: OrgMemberView[];
				roles: { id: string; name: string }[];
				grants: Record<string, string>;
		  };
	let dialog = $state<Dialog | null>(null);
	let busy = $state(false);
	// One org column / group header only once there is more than one org to tell apart.
	let multiOrg = $derived(data.ownedOrgs.length > 1);

	let q = $state('');
	let orgFilter = $state('');
	const sort = new TableSort<(typeof data.managed)[number]>({
		name: { by: (s) => s.name },
		org: { by: (s) => s.orgName },
		target: { by: (s) => `${s.host}:${s.port}` },
		order: { by: (s) => s.sortOrder }
	});
	let shown = $derived(
		sort.sorted(
			data.managed.filter(
				(s) =>
					(!orgFilter || s.orgId === orgFilter) &&
					matches(q, s.name, `${s.host}:${s.port}`, s.orgName, s.notes)
			)
		)
	);
	let filtering = $derived(!!q.trim() || !!orgFilter);
	/** where a new server goes by default: the header scope when it is an owned org, else the first */
	let preferredOrg = $derived(
		data.ownedOrgs.find((o) => o.id === data.scope?.id)?.id ?? data.ownedOrgs[0]?.id ?? ''
	);

	const openEdit = (s: ServerInfo | null) => {
		dialog = {
			kind: 'edit',
			server: s,
			orgId: s?.orgId ?? (orgFilter || preferredOrg),
			name: s?.name ?? '',
			host: s?.host ?? '',
			port: s ? String(s.port) : '',
			scheme: s?.scheme ?? 'http',
			password: '',
			notes: s?.notes ?? '',
			sortOrder: String(s?.sortOrder ?? 0),
			publicStatus: s?.publicStatus ?? false,
			publicLeaderboards: s?.publicLeaderboards ?? false
		};
	};
	/** An edit that changes where RCON listens is the add flow again: the password is asked for. */
	const moved = (d: { server: ServerInfo | null; host: string; port: string; scheme: string }) =>
		!!d.server &&
		(d.host.trim().toLowerCase() !== d.server.host ||
			Number(d.port) !== d.server.port ||
			d.scheme !== d.server.scheme);
	/** What the site owner allows the dialog's organisation (the server's, or the one picked for a new one). */
	const allowancesOf = (d: { server: ServerInfo | null; orgId: string }) =>
		d.server ?? data.ownedOrgs.find((o) => o.id === d.orgId) ?? NO_ALLOWANCES;
	const FEATURE_KEY = { status: 'publicStatus', leaderboards: 'publicLeaderboards' } as const;

	async function run(fn: () => Promise<void>, done: string) {
		busy = true;
		try {
			await fn();
			toast(done, 'ok');
			dialog = null;
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	function save() {
		const d = dialog;
		if (!d || d.kind !== 'edit') return;
		const payload: Record<string, unknown> = {
			name: d.name,
			host: d.host,
			port: Number(d.port),
			scheme: d.scheme,
			notes: d.notes,
			sortOrder: Number(d.sortOrder) || 0
		};
		// A switch the org is not allowed is left out: it stays as it was and the API never sees it.
		for (const feature of PUBLIC_FEATURES)
			if (featureState(allowancesOf(d), d, feature).allowed)
				payload[FEATURE_KEY[feature]] = d[FEATURE_KEY[feature]];
		if (d.password) payload.password = d.password;
		if (d.server) {
			const id = d.server.id;
			void run(() => api('PATCH', `/api/servers/${id}`, payload), '服务器已更新。');
		} else {
			payload.orgId = d.orgId;
			void run(() => api('POST', '/api/servers', payload), '服务器已添加。');
		}
	}

	async function test(s: ServerInfo) {
		const hide = toast(`Testing ${s.name}…`, '', 0);
		try {
			const d = await api<TestOk>('POST', `/api/servers/${s.id}/test`);
			dialog = { kind: 'test', server: s, result: d };
		} catch (err) {
			toast(`${s.name}: ${errorMessage(err)}`, 'err');
		} finally {
			hide();
		}
	}

	async function access(s: ServerInfo) {
		try {
			const orgPath = `/api/orgs/${encodeURIComponent(s.orgId)}`;
			const [res, grants, roles] = await Promise.all([
				api<{ members: OrgMemberView[] }>('GET', `${orgPath}/members`),
				api<{ grants: { userId: string; roleId: string }[] }>('GET', `/api/servers/${s.id}/grants`),
				api<{ roles: RoleView[] }>('GET', `${orgPath}/roles`)
			]);
			// Org owners and the site owner hold everything regardless of grants.
			const members = res.members.filter((u) => u.role !== 'owner' && !u.siteOwner);
			const map: Record<string, string> = {};
			for (const u of members)
				map[u.userId] = grants.grants.find((g) => g.userId === u.userId)?.roleId ?? '';
			dialog = {
				kind: 'access',
				server: s,
				members,
				roles: roles.roles.map((r) => ({ id: r.id, name: r.name })),
				grants: map
			};
		} catch (err) {
			toast(errorMessage(err), 'err');
		}
	}
	function saveAccess() {
		const d = dialog;
		if (!d || d.kind !== 'access') return;
		const grants = Object.entries(d.grants)
			.filter(([, roleId]) => roleId)
			.map(([userId, roleId]) => ({ userId, roleId }));
		void run(
			() => api('PUT', `/api/servers/${d.server.id}/grants`, { grants }),
			'访问权限已更新。'
		);
	}
	async function remove(s: ServerInfo) {
		if (
			!(await confirmDialog(
				`确定从面板移除“${s.name}”？游戏服务器本身不会受影响，审计历史会保留。`,
				{ okLabel: '移除', danger: true }
			))
		)
			return;
		await run(() => api('DELETE', `/api/servers/${s.id}`), '服务器已移除。');
	}
</script>

<svelte:head><title>服务器 · {data.appName}</title></svelte:head>

<div class="mb-5 flex items-center gap-3">
	<h1 class="text-xl font-semibold tracking-tight">服务器</h1>
	<button
		class="ml-auto btn btn-primary"
		onclick={() => openEdit(null)}
		disabled={!data.ownedOrgs.length}>添加服务器</button
	>
</div>

<div class="callout">
	每个条目对应一台 WARDOGS 专用服务器的 RCON 监听端口（配置文件中的 <code class="chip"
		>[/Script/WDRCON.WDRCONSettings]</code
	>
	区块）。RCON 密码会加密保存，之后不会再次显示。
	{#if data.demoAllowed}主机 <code class="chip">演示</code> 使用密码
		<code class="chip">演示</code> 使用内置模拟服务器。{/if}
	{#if !data.ownedOrgs.length}服务器必须归属某个组织。你还没有自己的组织，请先到“组织”页面创建。 <a
			href="/orgs"
			class="font-semibold text-accent underline">组织</a
		> 后再继续。{/if}
</div>

{#if data.managed.length > 5 || multiOrg}
	<div class="mb-3 flex flex-wrap items-center gap-2">
		<input
			class="input sm:w-72"
			type="search"
			placeholder="搜索名称、主机或备注…"
			bind:value={q}
			aria-label="搜索服务器"
		/>
		{#if multiOrg}
			<select class="input sm:w-56" bind:value={orgFilter} aria-label="组织">
				<option value="">全部组织</option>
				{#each data.ownedOrgs as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
			</select>
		{/if}
		{#if filtering}
			<span class="text-[12.5px] text-mist-400">{shown.length} of {data.managed.length}</span>
		{/if}
	</div>
{/if}

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<SortHeader {sort} key="name">名称</SortHeader>
				{#if multiOrg}<SortHeader {sort} key="org">组织</SortHeader>{/if}
				<SortHeader {sort} key="target">目标</SortHeader>
				<SortHeader {sort} key="order" num>顺序</SortHeader>
				<th></th>
			</tr>
		</thead>
		<tbody>
			{#each shown as s (s.id)}
				<tr>
					<td>
						<a
							href="/server/{encodeURIComponent(s.id)}"
							class="font-medium text-accent hover:underline">{s.name}</a
						>
						{#if s.demo}<Badge tone="info" class="ml-1">演示</Badge>{/if}
						{#if s.notes}<div class="text-[12px] text-mist-400">{s.notes}</div>{/if}
					</td>
					{#if multiOrg}<td
							><a href="/orgs/{encodeURIComponent(s.orgId)}" class="hover:underline">{s.orgName}</a
							></td
						>{/if}
					<td class="font-mono text-[12.5px]">{s.scheme}://{s.host}:{s.port}</td>
					<td class="num">{s.sortOrder}</td>
					<td class="text-right whitespace-nowrap">
						<span class="inline-flex gap-1.5">
							<button class="btn btn-sm" onclick={() => test(s)}>测试</button>
							<button class="btn btn-sm" onclick={() => openEdit(s)}>编辑</button>
							<button class="btn btn-sm" onclick={() => access(s)}>访问权限</button>
							<button class="btn btn-sm btn-danger" onclick={() => remove(s)}>删除</button>
						</span>
					</td>
				</tr>
			{:else}
				<tr
					><td colspan={multiOrg ? 5 : 4} class="py-8 text-center text-mist-600"
						>{filtering ? '没有符合条件的服务器。' : '还没有服务器。'}</td
					></tr
				>
			{/each}
		</tbody>
	</table>
</div>

{#if dialog?.kind === 'edit'}
	{@const d = dialog}
	<Modal title={d.server ? `编辑 ${d.server.name}` : '添加服务器'} onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				save();
			}}
		>
			{#if !d.server && multiOrg}
				<label class="block"
					><span class="field-label">组织</span>
					<select class="input" bind:value={d.orgId} required>
						{#each data.ownedOrgs as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
					</select>
				</label>
			{/if}
			<label class="block"
				><span class="field-label">名称</span><input
					class="input"
					type="text"
					bind:value={d.name}
					placeholder="显示名称"
					required
				/></label
			>
			<div class="grid grid-cols-2 gap-2 sm:grid-cols-[3fr_1fr_1fr]">
				<label class="col-span-2 block sm:col-span-1"
					><span class="field-label">主机地址</span><input
						class="input"
						type="text"
						bind:value={d.host}
						placeholder={data.user.role === 'owner' ? 'IP or hostname' : '公网 IP 或主机名'}
						spellcheck="false"
						required
					/></label
				>
				<label class="block"
					><span class="field-label">端口</span><input
						class="input"
						type="number"
						bind:value={d.port}
						placeholder="7776"
						min="1"
						max="65535"
						required
					/></label
				>
				<label class="block"
					><span class="field-label">协议</span>
					<select class="input" bind:value={d.scheme}
						><option value="http">http</option><option value="https">https</option></select
					>
				</label>
			</div>
			<label class="block"
				><span class="field-label">RCON 密码</span><input
					class="input"
					type="password"
					bind:value={d.password}
					placeholder={!d.server
						? 'RCON 密码'
						: moved(d)
							? '地址已变化，需要重新输入'
							: '(unchanged)'}
					autocomplete="new-password"
					required={!d.server || moved(d)}
				/></label
			>
			<div class="grid grid-cols-1 gap-2 sm:grid-cols-[4fr_1fr]">
				<label class="block"
					><span class="field-label">备注</span><textarea
						class="input"
						rows="2"
						bind:value={d.notes}
						placeholder="所有有权限者可见的备注（可选）"></textarea></label
				>
				<label class="block"
					><span class="field-label">排序顺序</span><input
						class="input"
						type="number"
						bind:value={d.sortOrder}
					/></label
				>
			</div>
			<div>
				<span class="field-label">公开页面</span>
				{#each PUBLIC_FEATURES as feature (feature)}
					{@const st = featureState(allowancesOf(d), d, feature)}
					<label
						class="flex items-center gap-2 py-0.5 text-[13px] {st.allowed ? '' : 'opacity-50'}"
						title={st.reason ?? undefined}
					>
						<input type="checkbox" bind:checked={d[FEATURE_KEY[feature]]} disabled={!st.allowed} />
						{FEATURE_LABELS[feature]}
						{#if st.reason}<span class="text-[12px] text-mist-600">· {st.reason}</span>{/if}
					</label>
				{/each}
			</div>
			<p class="note">
				{#if data.user.role === 'owner'}
					Warcon 所在主机必须能连接此端口：若与游戏服务器同机，可使用 BindAddress
					127.0.0.1；否则需在防火墙或反向代理后使用
					0.0.0.0。站点所有者可以添加私有地址；其他用户添加的服务器必须有公开地址。
				{:else}
					此主机必须可从公网访问：可在允许 Warcon 连接的防火墙后使用 BindAddress
					0.0.0.0，或通过反向代理暴露。私有和内部地址会被拒绝。若游戏服务器与 Warcon
					位于同一主机或网络，请联系站点所有者添加。
				{/if}
				使用 HTTPS 时需要 Warcon 信任的证书；自签名证书需设置 GAME_TLS_INSECURE=true。
			</p>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}
					>{d.server ? '保存' : '添加'}</button
				>
			</div>
		</form>
	</Modal>
{:else if dialog?.kind === 'test'}
	{@const d = dialog}
	<Modal
		title="{d.server.name} — reachable in {d.result.durationMs} ms"
		onclose={() => (dialog = null)}
	>
		<div class="kv">
			<span class="text-mist-400">服务器名称</span><span>{d.result.status.serverName}</span>
		</div>
		<div class="kv"><span class="text-mist-400">地图</span><span>{d.result.status.map}</span></div>
		<div class="kv">
			<span class="text-mist-400">玩家</span><span
				>{fmtNum(d.result.status.playerCount)} / {fmtNum(d.result.status.maxPlayers)}</span
			>
		</div>
		{#if d.result.status.raw}
			<details class="mt-2">
				<summary class="cursor-pointer text-[12.5px] text-mist-400">服务器原始状态</summary>
				<pre
					class="mt-2 max-h-56 overflow-auto rounded-card border border-black bg-ink-950 p-3 font-mono text-[12px] leading-relaxed">{JSON.stringify(
						d.result.status.raw,
						null,
						2
					)}</pre>
			</details>
		{/if}
		{#if d.result.capabilities?.raw?.build}
			<div class="kv">
				<span class="text-mist-400">版本</span>
				<span class="text-right font-mono">{String(d.result.capabilities.raw.build)}</span>
			</div>
		{/if}
		{#if d.result.serverId}
			<div class="kv">
				<span class="text-mist-400">加入代码</span>
				<span class="text-right font-mono">{d.result.serverId}</span>
			</div>
		{/if}
		<div class="kv">
			<span class="text-mist-400">权限项</span>
			<span class="text-right">
				{#if d.result.capabilities}{d.result.capabilities.routes.length} 路由；切换阵营 {d.result
						.capabilities.features.changeTeam
						? 'yes'
						: 'no'}；配置文档 {d.result.capabilities.features.configDocument
						? 'yes'
						: 'no'}{:else}旧版插件未上报{/if}
			</span>
		</div>
		{#if d.result.capabilities?.raw}
			{@const { routes: _routes, ...rest } = d.result.capabilities.raw}
			{#if Object.keys(rest).length}
				<pre
					class="mt-2 max-h-40 overflow-auto rounded-card border border-black bg-ink-950 p-3 font-mono text-[12px] leading-relaxed">{JSON.stringify(
						rest,
						null,
						2
					)}</pre>
			{/if}
		{/if}
		{#if d.result.capabilities?.routes.length}
			<details class="mt-2">
				<summary class="cursor-pointer text-[12.5px] text-mist-400">当前版本提供的路由</summary>
				<ul class="mt-2 max-h-56 overflow-y-auto font-mono text-[12px] leading-relaxed">
					{#each d.result.capabilities.routes as r (r)}<li>{r}</li>{/each}
				</ul>
			</details>
		{/if}
		{#snippet actions()}<button type="button" class="btn" onclick={() => (dialog = null)}
				>关闭</button
			>{/snippet}
	</Modal>
{:else if dialog?.kind === 'access'}
	{@const d = dialog}
	<Modal title="Access to {d.server.name}" onclose={() => (dialog = null)}>
		{#if d.members.length}
			<GrantList
				rows={d.members.map((u) => ({
					id: u.userId,
					label: u.name || u.username,
					sub: `@${u.username}`
				}))}
				roles={d.roles}
				bind:grants={d.grants}
			/>
			<p class="note">
				以下组织的所有者： {d.server.orgName} 无论设置如何，所有者始终拥有全部权限。一次设置整个组织：
				<a href="/orgs/{encodeURIComponent(d.server.orgId)}/access" class="text-accent underline"
					>权限矩阵</a
				>.
			</p>
		{:else}
			<p class="text-mist-400">
				除所有者外暂无成员。请到组织页面分享邀请链接。 <a
					href="/orgs/{encodeURIComponent(d.server.orgId)}"
					class="text-accent underline">{d.server.orgName}</a
				>。所有者始终拥有访问权限。
			</p>
		{/if}
		{#snippet actions()}
			<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
			<button type="button" class="btn btn-primary" onclick={saveAccess} disabled={busy}
				>保存权限</button
			>
		{/snippet}
	</Modal>
{/if}
