<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import GrantList from '$lib/components/GrantList.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import type { UserView } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	type Dialog =
		| {
				kind: 'edit';
				user: UserView | null;
				username: string;
				displayName: string;
				password: string;
				role: 'owner' | 'member';
				disabled: boolean;
				mustChange: boolean;
		  }
		| { kind: 'grants'; user: UserView; grants: Record<string, string> }
		| {
				kind: 'reset';
				user: UserView;
				password: string;
				mustChange: boolean;
				/** lost device: also drop their authenticator app, passkeys and recovery key */
				resetAuth: boolean;
		  };
	let dialog = $state<Dialog | null>(null);
	let busy = $state(false);

	let search = $state('');
	const status = (u: UserView) => (u.disabled ? 2 : u.mustChangePassword ? 1 : 0);
	const sort = new TableSort<UserView>({
		user: { by: (u) => u.name || u.username },
		role: { by: (u) => u.role },
		status: { by: status },
		signIn: { by: (u) => (u.authComplete ? 1 : 0) },
		orgs: { by: (u) => u.orgs.length, dir: 'desc' },
		access: { by: (u) => (u.role === 'owner' ? Infinity : u.grants.length), dir: 'desc' },
		lastLogin: { by: (u) => u.lastLoginAt, dir: 'desc' }
	});
	let rows = $derived(
		sort.sorted(
			data.users.filter((u) => matches(search, u.name, u.username, ...u.orgs.map((o) => o.orgName)))
		)
	);

	const openEdit = (u: UserView | null) => {
		dialog = {
			kind: 'edit',
			user: u,
			username: u?.username ?? '',
			displayName: u?.name ?? '',
			password: '',
			role: u?.role ?? 'member',
			disabled: !!u?.disabled,
			mustChange: u ? u.mustChangePassword : true
		};
	};
	/** "admin 2 · viewer 3": one number per role name rather than one chip per server */
	const grantSummary = (u: UserView) => {
		const counts = new Map<string, number>();
		for (const g of u.grants) counts.set(g.roleName, (counts.get(g.roleName) ?? 0) + 1);
		return [...counts].map(([name, n]) => `${name} ${n}`).join(' · ');
	};
	const openGrants = (u: UserView) => {
		const grants: Record<string, string> = {};
		for (const s of data.servers)
			grants[s.id] = u.grants.find((g) => g.serverId === s.id)?.roleId ?? '';
		dialog = { kind: 'grants', user: u, grants };
	};
	const openReset = (u: UserView) => {
		dialog = { kind: 'reset', user: u, password: '', mustChange: true, resetAuth: false };
	};

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
		if (d.user) {
			const id = d.user.id;
			void run(
				() =>
					api('PATCH', `/api/users/${id}`, {
						displayName: d.displayName,
						role: d.role,
						disabled: d.disabled,
						mustChangePassword: d.mustChange
					}),
				'用户已更新。'
			);
		} else {
			void run(
				() =>
					api('POST', '/api/users', {
						username: d.username.trim(),
						displayName: d.displayName,
						password: d.password,
						role: d.role,
						mustChangePassword: d.mustChange
					}),
				'用户已创建。'
			);
		}
	}
	function saveGrants() {
		const d = dialog;
		if (!d || d.kind !== 'grants') return;
		const grants = Object.entries(d.grants)
			.filter(([, roleId]) => roleId)
			.map(([serverId, roleId]) => ({ serverId, roleId }));
		void run(() => api('PUT', `/api/users/${d.user.id}/grants`, { grants }), '访问权限已更新。');
	}
	function reset() {
		const d = dialog;
		if (!d || d.kind !== 'reset') return;
		void run(
			() =>
				api('PATCH', `/api/users/${d.user.id}`, {
					password: d.password,
					mustChangePassword: d.mustChange,
					resetAuth: d.resetAuth
				}),
			d.resetAuth ? '登录方式已重置。' : '密码已重置。'
		);
	}
	async function remove(u: UserView) {
		if (
			!(await confirmDialog(
				`Delete @${u.username}? Their audit history is kept, their access is removed.`,
				{ okLabel: 'Delete', danger: true }
			))
		)
			return;
		await run(() => api('DELETE', `/api/users/${u.id}`), '用户已删除。');
	}
</script>

<svelte:head><title>用户 · 站点管理 · {data.appName}</title></svelte:head>

<div class="mb-4 flex items-center gap-3">
	<button class="ml-auto btn btn-primary" onclick={() => openEdit(null)}>添加用户</button>
</div>

<div class="callout">
	<b>本面板上的所有账号。</b> 一个站点 <b>所有者</b> 管理整个平台，可以操作所有服务器。 <b>成员</b>
	属于一个或多个
	<a href="/orgs" class="text-accent underline">组织</a
	>通常通过邀请链接加入组织，并以组织分配的角色访问获授权的服务器。每个组织初始提供 <b>查看者</b>
	（只读）， <b>操作员</b> （踢人、处决、私信、广播、地图及对局控制、实时轮换编辑、备注）和
	<b>管理员</b> （拥有服务器全部权限）；组织所有者可调整权限并添加自定义角色。在此授予服务器权限时，该用户也会加入所属组织。
</div>

<div class="mb-3 flex flex-wrap items-center gap-2">
	<input
		class="input w-full sm:w-80"
		type="search"
		placeholder="按名称、用户名或组织筛选…"
		aria-label="筛选用户"
		bind:value={search}
	/>
	<span class="text-[12.5px] text-mist-600">显示 {rows.length} / {data.users.length} 位用户</span>
</div>

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<SortHeader {sort} key="user">用户</SortHeader>
				<SortHeader {sort} key="role">角色</SortHeader>
				<SortHeader {sort} key="status">状态</SortHeader>
				<SortHeader {sort} key="signIn">登录</SortHeader>
				<SortHeader {sort} key="orgs">组织</SortHeader>
				<SortHeader {sort} key="access">服务器权限</SortHeader>
				<SortHeader {sort} key="lastLogin">上次登录</SortHeader>
				<th></th>
			</tr>
		</thead>
		<tbody>
			{#each rows as u (u.id)}
				<tr>
					<td>
						<div>{u.name || u.username}</div>
						<div class="font-mono text-[12px] text-mist-600">@{u.username}</div>
					</td>
					<td><RoleBadge role={u.role} /></td>
					<td>
						{#if u.disabled}<Badge tone="err">已停用</Badge>{:else if u.mustChangePassword}<Badge
								tone="info">必须修改密码</Badge
							>{:else}<Badge tone="ok">启用中</Badge>{/if}
					</td>
					<td>
						<div class="flex flex-wrap items-center gap-1.5 text-[12px] text-mist-400">
							{#if u.signIn.length}{u.signIn.join(' · ')}{:else}<span class="text-mist-600">无</span
								>{/if}
							{#if !u.authComplete}<Badge tone="warn">未完成</Badge>{/if}
						</div>
					</td>
					<td>
						{#if u.orgs.length}
							<div class="flex flex-wrap gap-1.5">
								{#each u.orgs as o (o.orgId)}
									<span
										class="inline-flex items-center gap-1.5 rounded-[2px] border border-black bg-ink-950 py-0.5 pr-1 pl-2 text-[12px]"
										>{o.orgName} <RoleBadge role={o.role} /></span
									>
								{/each}
							</div>
						{:else}
							<span class="text-mist-600">无</span>
						{/if}
					</td>
					<td>
						{#if u.role === 'owner'}
							<span class="text-mist-400">所有服务器（站点所有者）</span>
						{:else if u.grants.length}
							<button
								type="button"
								class="block text-left whitespace-nowrap hover:underline"
								title={u.grants.map((g) => `${g.serverName}: ${g.roleName}`).join('\n')}
								onclick={() => openGrants(u)}
							>
								<div>
									已授权 {u.grants.length} / {data.servers.length} 台服务器
								</div>
								<div class="text-[12px] text-mist-400">{grantSummary(u)}</div>
							</button>
						{:else}
							<span class="text-mist-600">无</span>
						{/if}
					</td>
					<td class="whitespace-nowrap text-mist-400">{fmtTime(u.lastLoginAt)}</td>
					<td class="text-right whitespace-nowrap">
						<span class="inline-flex gap-1.5">
							<button class="btn btn-sm" onclick={() => openEdit(u)}>编辑</button>
							{#if u.role !== 'owner'}<button class="btn btn-sm" onclick={() => openGrants(u)}
									>访问权限</button
								>{/if}
							<button class="btn btn-sm" onclick={() => openReset(u)}>重置登录方式</button>
							{#if u.id !== data.user.id}<button
									class="btn btn-sm btn-danger"
									onclick={() => remove(u)}>删除</button
								>{/if}
						</span>
					</td>
				</tr>
			{:else}
				<tr><td colspan="7" class="py-6 text-center text-mist-600">没有符合条件的玩家。</td></tr>
			{/each}
		</tbody>
	</table>
</div>

{#if dialog?.kind === 'edit'}
	{@const d = dialog}
	<Modal title={d.user ? `编辑 @${d.user.username}` : '添加用户'} onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				save();
			}}
		>
			<label class="block"
				><span class="field-label">用户名</span><input
					class="input"
					type="text"
					bind:value={d.username}
					disabled={!!d.user}
					autocomplete="off"
					spellcheck="false"
					required
				/></label
			>
			<label class="block"
				><span class="field-label">显示名称</span><input
					class="input"
					type="text"
					bind:value={d.displayName}
				/></label
			>
			{#if !d.user}
				<label class="block"
					><span class="field-label">初始密码（至少 10 位）</span><input
						class="input"
						type="password"
						bind:value={d.password}
						autocomplete="new-password"
						minlength="10"
						required
					/></label
				>
			{/if}
			<label class="block"
				><span class="field-label">全局角色</span>
				<select class="input" bind:value={d.role}
					><option value="member">成员</option><option value="owner">站点所有者</option></select
				>
			</label>
			<div class="flex flex-wrap gap-5 pt-1 text-[13px]">
				{#if d.user}<label class="inline-flex items-center gap-2"
						><input type="checkbox" bind:checked={d.disabled} /> 停用（同时退出所有会话）</label
					>{/if}
				<label class="inline-flex items-center gap-2"
					><input type="checkbox" bind:checked={d.mustChange} /> 下次登录时必须修改密码</label
				>
			</div>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}
					>{d.user ? '保存' : '创建'}</button
				>
			</div>
		</form>
	</Modal>
{:else if dialog?.kind === 'grants'}
	{@const d = dialog}
	<Modal title="Server access for @{d.user.username}" onclose={() => (dialog = null)}>
		<GrantList
			rows={data.servers.map((s) => ({
				id: s.id,
				label: s.name,
				sub: data.orgs.length > 1 ? `${s.orgName} · ${s.host}:${s.port}` : `${s.host}:${s.port}`,
				roles: data.rolesByOrgId[s.orgId] ?? []
			}))}
			bind:grants={d.grants}
			empty="No servers exist yet."
		/>
		{#snippet actions()}
			<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
			<button type="button" class="btn btn-primary" onclick={saveGrants} disabled={busy}
				>保存权限</button
			>
		{/snippet}
	</Modal>
{:else if dialog?.kind === 'reset'}
	{@const d = dialog}
	<Modal title="Reset sign-in for @{d.user.username}" onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				reset();
			}}
		>
			<label class="block"
				><span class="field-label">新密码（至少 10 位）</span><input
					class="input"
					type="password"
					bind:value={d.password}
					autocomplete="new-password"
					minlength="10"
					required
				/></label
			>
			<label class="inline-flex items-center gap-2 text-[13px]"
				><input type="checkbox" bind:checked={d.mustChange} /> 要求下次登录时设置新密码</label
			>
			<label class="flex items-start gap-2 text-[13px]"
				><input type="checkbox" class="mt-0.5" bind:checked={d.resetAuth} />
				<span
					>设备丢失：同时移除其验证器、所有通行密钥和恢复密钥。已关联的 Discord 和 Steam
					账号会保留。</span
				></label
			>
			<p class="note">该用户的所有会话已退出。请通过其他渠道告知新密码。</p>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}>重置</button>
			</div>
		</form>
	</Modal>
{/if}
