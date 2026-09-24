<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import CapabilityPicker from '$lib/components/CapabilityPicker.svelte';
	import SortHeader from '$lib/components/SortHeader.svelte';
	import { TableSort, matches } from '$lib/table.svelte';
	import { capabilitySummary, type Capability } from '$lib/capabilities';
	import type { ApiKeyView, InviteView, OrgMemberView, WebhookView } from '$lib/types';
	import { STATUS_STYLE_LABELS, STATUS_STYLES, type StatusStyle } from '$lib/status-styles';
	import CardOptions from '$lib/components/CardOptions.svelte';
	import { FEATURE_LABELS, PUBLIC_FEATURES, allowed } from '$lib/features';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let memberSearch = $state('');
	const memberSort = new TableSort<OrgMemberView>({
		member: { by: (m) => m.name || m.username },
		role: { by: (m) => m.role },
		access: { by: (m) => (m.role === 'owner' ? Infinity : m.grants.length), dir: 'desc' },
		joined: { by: (m) => m.joinedAt, dir: 'desc' }
	});
	let members = $derived(
		memberSort.sorted(data.members.filter((m) => matches(memberSearch, m.name, m.username)))
	);

	type Dialog =
		| {
				kind: 'invite';
				label: string;
				orgRole: 'owner' | 'member';
				serverRoleId: string;
				expiresDays: string;
				maxUses: string;
		  }
		| { kind: 'created'; invite: InviteView }
		| {
				kind: 'webhook';
				id: string | null;
				label: string;
				url: string;
				events: Record<string, boolean>;
				status: boolean;
				style: StatusStyle;
				interval: number;
				linkStatus: boolean;
				linkLeaderboard: boolean;
				linkPanel: boolean;
				allServers: boolean;
				servers: Record<string, boolean>;
		  }
		| {
				kind: 'key';
				label: string;
				capabilities: Capability[];
				allServers: boolean;
				servers: Record<string, boolean>;
				expiresDays: string;
		  }
		| { kind: 'keyCreated'; key: ApiKeyView; token: string };
	let dialog = $state<Dialog | null>(null);
	let busy = $state(false);

	let orgPath = $derived(`/api/orgs/${encodeURIComponent(data.org.id)}`);

	async function run(fn: () => Promise<void>, done: string, close = true) {
		busy = true;
		try {
			await fn();
			if (done) toast(done, 'ok');
			if (close) dialog = null;
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function copy(text: string, what = '邀请链接') {
		try {
			await navigator.clipboard.writeText(text);
			toast(`${what} copied.`, 'ok');
		} catch {
			window.prompt(`Copy the ${what.toLowerCase()}:`, text);
		}
	}

	// --- API keys (bots) ---
	const openKey = () => {
		const servers: Record<string, boolean> = {};
		for (const s of data.orgServers) servers[s.id] = true;
		dialog = {
			kind: 'key',
			label: '',
			capabilities: ['server.view'],
			allServers: true,
			servers,
			expiresDays: ''
		};
	};
	function createKey() {
		const d = dialog;
		if (!d || d.kind !== 'key') return;
		void run(
			async () => {
				const res = await api<{ key: ApiKeyView; token: string }>('POST', `${orgPath}/keys`, {
					label: d.label,
					capabilities: d.capabilities,
					serverIds: d.allServers
						? null
						: data.orgServers.filter((s) => d.servers[s.id]).map((s) => s.id),
					expiresDays: d.expiresDays ? Number(d.expiresDays) : null
				});
				dialog = { kind: 'keyCreated', key: res.key, token: res.token };
			},
			'',
			false
		);
	}
	async function revokeKey(k: ApiKeyView) {
		if (
			!(await confirmDialog(`确定撤销“${k.label}”密钥？依赖它的程序会立即失效。`, {
				okLabel: '撤销',
				danger: true
			}))
		)
			return;
		await run(() => api('DELETE', `${orgPath}/keys/${k.id}`), 'API 密钥已撤销。', false);
	}
	const keyServers = (k: ApiKeyView) =>
		k.serverIds === null
			? '每台服务器'
			: k.serverIds.map((id) => data.orgServers.find((s) => s.id === id)?.name ?? '?').join(', ');

	const openInvite = () => {
		dialog = {
			kind: 'invite',
			label: '',
			orgRole: 'member',
			serverRoleId: data.roles.find((r) => r.builtin === 'viewer')?.id ?? '',
			expiresDays: '7',
			maxUses: ''
		};
	};
	function createInvite() {
		const d = dialog;
		if (!d || d.kind !== 'invite') return;
		void run(
			async () => {
				const res = await api<{ invite: InviteView }>('POST', `${orgPath}/invites`, {
					label: d.label,
					orgRole: d.orgRole,
					serverRoleId: d.serverRoleId || null,
					expiresDays: d.expiresDays ? Number(d.expiresDays) : null,
					maxUses: d.maxUses ? Number(d.maxUses) : null
				});
				dialog = { kind: 'created', invite: res.invite };
			},
			'',
			false
		);
	}
	async function revoke(inv: InviteView) {
		if (
			!(await confirmDialog('确定撤销此邀请链接？已加入的成员仍保留访问权限。', {
				okLabel: 'Revoke',
				danger: true
			}))
		)
			return;
		await run(() => api('DELETE', `${orgPath}/invites/${inv.id}`), '邀请链接已撤销。');
	}

	function setRole(m: OrgMemberView, role: string) {
		void run(
			() => api('PATCH', `${orgPath}/members/${m.userId}`, { role }),
			`@${m.username} is now ${role}.`
		);
	}
	let accessHref = $derived(`/orgs/${encodeURIComponent(data.org.id)}/access`);
	/** "admin 2 · viewer 3": one number per role rather than one chip per server */
	const grantSummary = (m: OrgMemberView) =>
		data.roles
			.filter((r) => m.grants.some((g) => g.roleId === r.id))
			.map((r) => `${r.name} ${m.grants.filter((g) => g.roleId === r.id).length}`)
			.join(' · ');
	const builtinOf = (roleId: string | null) =>
		data.roles.find((r) => r.id === roleId)?.builtin ?? null;
	async function remove(m: OrgMemberView) {
		if (
			!(await confirmDialog(
				`确定从“${data.org.name}”移除 @${m.username}？其服务器访问权限会被撤销，账号仍会保留。`,
				{ okLabel: '移除', danger: true }
			))
		)
			return;
		await run(() => api('DELETE', `${orgPath}/members/${m.userId}`), '成员已移除。');
	}

	const STATUS_BADGE: Record<InviteView['status'], { tone: 'ok' | 'warn' | 'err'; text: string }> =
		{
			live: { tone: 'ok', text: 'live' },
			revoked: { tone: 'err', text: 'revoked' },
			expired: { tone: 'warn', text: 'expired' },
			used: { tone: 'warn', text: 'used up' }
		};
	const usesLabel = (inv: InviteView) =>
		inv.maxUses === null ? `${inv.uses}` : `${inv.uses} / ${inv.maxUses}`;

	// --- Discord webhooks ---
	const openWebhook = (w: WebhookView | null) => {
		const events: Record<string, boolean> = {};
		for (const e of data.webhookEvents)
			events[e.key] = w
				? w.events.includes(e.key)
				: e.key === 'bans' || e.key === 'commands' || e.key === 'triggers';
		const servers: Record<string, boolean> = {};
		for (const s of data.orgServers) servers[s.id] = !!w?.serverIds?.includes(s.id);
		dialog = {
			kind: 'webhook',
			id: w?.id ?? null,
			label: w?.label ?? '',
			url: '',
			events,
			status: w?.statusEnabled ?? false,
			style: w?.statusStyle ?? 'banner',
			interval: w?.statusIntervalS ?? 60,
			linkStatus: w?.linkStatus ?? true,
			linkLeaderboard: w?.linkLeaderboard ?? true,
			linkPanel: w?.linkPanel ?? false,
			allServers: !w?.serverIds,
			servers
		};
	};
	function saveWebhook() {
		const d = dialog;
		if (!d || d.kind !== 'webhook') return;
		const body: Record<string, unknown> = {
			label: d.label.trim(),
			events: Object.entries(d.events)
				.filter(([, on]) => on)
				.map(([k]) => k),
			statusEnabled: d.status,
			statusStyle: d.style,
			statusIntervalS: d.interval,
			linkStatus: d.linkStatus,
			linkLeaderboard: d.linkLeaderboard,
			linkPanel: d.linkPanel,
			serverIds: d.allServers
				? null
				: Object.entries(d.servers)
						.filter(([, on]) => on)
						.map(([k]) => k)
		};
		if (d.url.trim()) body.url = d.url.trim();
		void run(
			() =>
				d.id
					? api('PATCH', `${orgPath}/webhooks/${d.id}`, body)
					: api('POST', `${orgPath}/webhooks`, body),
			d.id ? '网络钩子已更新。' : '网络钩子已添加。'
		);
	}
	function toggleWebhook(w: WebhookView) {
		void run(
			() => api('PATCH', `${orgPath}/webhooks/${w.id}`, { enabled: !w.enabled }),
			w.enabled ? '网络钩子已暂停。' : '网络钩子已启用。',
			false
		);
	}
	function testWebhook(w: WebhookView) {
		void run(() => api('POST', `${orgPath}/webhooks/${w.id}/test`), '测试消息已发送。', false);
	}
	/** A throwaway card for the first server the webhook covers, gone in a minute. */
	function testCard(w: WebhookView) {
		const serverId = w.serverIds?.[0] ?? data.orgServers[0]?.id;
		if (!serverId) return;
		void run(
			() => api('POST', `${orgPath}/webhooks/${w.id}/card`, { serverId }),
			'测试卡片已发送，将在一分钟后消失。',
			false
		);
	}
	async function deleteWebhook(w: WebhookView) {
		if (
			!(await confirmDialog(`Remove the ${w.label} webhook?`, { okLabel: 'Remove', danger: true }))
		)
			return;
		await run(() => api('DELETE', `${orgPath}/webhooks/${w.id}`), '网络钩子已移除。', false);
	}
	const eventLabel = (key: string) =>
		data.webhookEvents.find((e) => e.key === key)?.label.split(' (')[0] ?? key;

	// --- site owner controls ---
	// A number input binds a number, or null when blank (blank = the instance default).
	let limitInput = $state<number | null>(null);
	let suspendReason = $state('');
	$effect(() => {
		limitInput = data.org.customServerLimit;
	});
	function saveLimit() {
		void run(
			() => api('PATCH', orgPath, { serverLimit: limitInput }),
			'服务器数量上限已更新。',
			false
		);
	}
	async function suspend() {
		if (
			!(await confirmDialog(
				`Suspend ${data.org.name}? Members lose access to its servers and its invite links stop working until you restore it.`,
				{ okLabel: 'Suspend', danger: true }
			))
		)
			return;
		await run(
			() => api('PATCH', orgPath, { suspended: true, reason: suspendReason.trim() }),
			'组织已暂停。',
			false
		);
	}
	function restore() {
		void run(() => api('PATCH', orgPath, { suspended: false }), '组织已恢复。', false);
	}
	const ALLOW_KEY = {
		status: 'allowPublicStatus',
		leaderboards: 'allowPublicLeaderboards'
	} as const;
	const setAllowance = (feature: 'status' | 'leaderboards', on: boolean) =>
		run(
			() => api('PATCH', orgPath, { [ALLOW_KEY[feature]]: on }),
			on ? `${FEATURE_LABELS[feature]} allowed.` : `${FEATURE_LABELS[feature]} no longer allowed.`,
			false
		);

	// --- the org's public pages: the Discord invite shown on them ---
	let inviteUrl = $state('');
	$effect(() => {
		inviteUrl = data.org.discordInviteUrl;
	});
	function saveInvite() {
		void run(
			() => api('PATCH', orgPath, { discordInviteUrl: inviteUrl.trim() }),
			inviteUrl.trim() ? 'Discord 邀请链接已保存。' : 'Discord 邀请链接已移除。',
			false
		);
	}
	let anyAllowed = $derived(PUBLIC_FEATURES.some((f) => allowed(data.org, f)));
</script>

<div class="grid grid-cols-1 gap-4 xl:grid-cols-[3fr_2fr]">
	<div class="space-y-4">
		<div class="panel">
			<div class="mb-3 flex items-center gap-3">
				<span class="label-sm mb-0!">邀请链接</span>
				<button class="ml-auto btn btn-sm btn-primary" onclick={openInvite}>新邀请链接</button>
			</div>
			<p class="mb-3 text-[13px] text-mist-400">
				把邀请链接发到 Discord。打开链接的人可用 Discord 或已有用户名登录，并按下方设置加入组织。
				{#if !data.discord}<span class="text-warn"
						>尚未配置 Discord 登录，因此只有已注册的用户能使用邀请链接。</span
					>{/if}
			</p>
			<div class="table-wrap">
				<table>
					<thead
						><tr
							><th>标签</th><th>加入时的角色</th><th class="num">使用次数</th><th>到期时间</th><th
								>状态</th
							><th></th></tr
						></thead
					>
					<tbody>
						{#each data.invites as inv (inv.id)}
							{@const status = inv.status}
							<tr class={status === 'live' ? '' : 'text-mist-600'}>
								<td>
									<div>{inv.label || '—'}</div>
									<div class="font-mono text-[11px] text-mist-600">
										{fmtTime(inv.createdAt)}
									</div>
								</td>
								<td>
									<span class="inline-flex flex-wrap items-center gap-1">
										<RoleBadge role={inv.orgRole} />
										{#if inv.serverRoleName}<RoleBadge
												role={inv.serverRoleName}
												builtin={builtinOf(inv.serverRoleId)}
											/>{:else}<Badge>没有服务器</Badge>{/if}
									</span>
								</td>
								<td class="num">{usesLabel(inv)}</td>
								<td class="whitespace-nowrap">{inv.expiresAt ? fmtTime(inv.expiresAt) : 'never'}</td
								>
								<td>
									<Badge tone={STATUS_BADGE[status].tone}>{STATUS_BADGE[status].text}</Badge>
								</td>
								<td class="text-right whitespace-nowrap">
									<span class="inline-flex gap-1.5">
										{#if status === 'live'}
											<button class="btn btn-sm" onclick={() => copy(inv.url)}>复制链接</button>
											<button class="btn btn-sm btn-danger" onclick={() => revoke(inv)}>撤销</button
											>
										{/if}
									</span>
								</td>
							</tr>
						{:else}
							<tr><td colspan="6" class="py-6 text-center text-mist-600">暂无邀请链接。</td></tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>

		<div class="panel">
			<div class="mb-3 flex flex-wrap items-center gap-2">
				<span class="label-sm mb-0!">成员</span>
				<input
					class="input w-full sm:ml-auto sm:w-64"
					type="search"
					placeholder="按名称或用户名筛选…"
					aria-label="筛选成员"
					bind:value={memberSearch}
				/>
			</div>
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							<SortHeader sort={memberSort} key="member">成员</SortHeader>
							<SortHeader sort={memberSort} key="role">组织角色</SortHeader>
							<SortHeader sort={memberSort} key="access">服务器权限</SortHeader>
							<SortHeader sort={memberSort} key="joined">加入时间</SortHeader>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each members as m (m.userId)}
							<tr>
								<td>
									<div>
										{m.name || m.username}
										{#if m.siteOwner}<Badge tone="accent" class="ml-1">站点所有者</Badge>{/if}
										{#if m.disabled}<Badge tone="err" class="ml-1">已停用</Badge>{/if}
									</div>
									<div class="font-mono text-[12px] text-mist-600">@{m.username}</div>
								</td>
								<td>
									<select
										class="input w-32"
										value={m.role}
										disabled={busy}
										onchange={(e) => setRole(m, (e.currentTarget as HTMLSelectElement).value)}
									>
										<option value="member">成员</option>
										<option value="owner">所有者</option>
									</select>
								</td>
								<td>
									{#if m.role === 'owner'}
										<span class="text-mist-400">所有服务器（所有者）</span>
									{:else if m.grants.length}
										<a
											href={accessHref}
											class="block whitespace-nowrap hover:underline"
											title={m.grants.map((g) => `${g.serverName}: ${g.roleName}`).join('\n')}
										>
											<div>
												{m.grants.length} of {data.orgServers.length} 服务器{data.orgServers
													.length === 1
													? ''
													: 's'}
											</div>
											<div class="text-[12px] text-mist-400">{grantSummary(m)}</div>
										</a>
									{:else}
										<span class="text-mist-600">无</span>
									{/if}
								</td>
								<td class="whitespace-nowrap text-mist-400">{fmtTime(m.joinedAt)}</td>
								<td class="text-right whitespace-nowrap">
									<span class="inline-flex gap-1.5">
										{#if m.role !== 'owner'}
											<a class="btn btn-sm" href={accessHref}>访问权限</a>
										{/if}
										{#if m.userId !== data.user.id}
											<button class="btn btn-sm btn-danger" onclick={() => remove(m)}>移除</button>
										{/if}
									</span>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	</div>

	<div class="space-y-4 self-start">
		{#if data.user.role === 'owner'}
			<div class="panel border-accent/40">
				<span class="label-sm">站点所有者设置</span>
				<div class="kv">
					<span class="text-mist-400">创建时间</span>
					<span
						>{fmtTime(data.org.createdAt)}{#if data.org.createdBy}&nbsp;by @{data.org.createdBy
								.username}{/if}</span
					>
				</div>
				<div class="kv items-center">
					<span class="text-mist-400">服务器上限</span>
					<span class="join">
						<input
							class="input w-24 text-right"
							type="number"
							min="0"
							max="1000"
							bind:value={limitInput}
							placeholder="default"
							aria-label="服务器上限"
						/>
						<button type="button" class="btn btn-sm h-auto" onclick={saveLimit} disabled={busy}
							>保存</button
						>
					</span>
				</div>
				<p class="note">
					留空则使用实例默认值。当前为 {data.org.serverCount} of {data.org.serverLimit}.
				</p>
				<div class="mt-3 border-t border-white/8 pt-3">
					<span class="field-label">本组织可开启的公开页面</span>
					{#each PUBLIC_FEATURES as feature (feature)}
						<label class="flex items-center gap-2 py-1 text-[13px]">
							<input
								type="checkbox"
								checked={allowed(data.org, feature)}
								disabled={busy}
								onchange={(e) => setAllowance(feature, e.currentTarget.checked)}
							/>
							{FEATURE_LABELS[feature]}
						</label>
					{/each}
					<p class="note">
						默认允许：组织所有者可按服务器开启公开页面。取消勾选后，本组织的所有同类公开页面会立即关闭。
					</p>
				</div>
				<div class="mt-3 border-t border-white/8 pt-3">
					{#if data.org.suspended}
						<button type="button" class="btn btn-sm" onclick={restore} disabled={busy}
							>恢复组织</button
						>
					{:else}
						<div class="join w-full">
							<input
								class="input"
								type="text"
								bind:value={suspendReason}
								placeholder="原因（组织所有者可见）"
								maxlength="300"
							/>
							<button
								type="button"
								class="btn btn-sm h-auto btn-danger"
								onclick={suspend}
								disabled={busy}>停用</button
							>
						</div>
					{/if}
				</div>
			</div>
		{/if}

		<div class="panel">
			<span class="label-sm">公开页面</span>
			{#if anyAllowed}
				<p class="mb-3 text-[13px] text-mist-400">
					本组织可以开放 {PUBLIC_FEATURES.filter((f) => allowed(data.org, f))
						.map((f) => FEATURE_LABELS[f].toLowerCase())
						.join(' and ')}。请到各服务器的 <b>设置</b> 标签页或编辑对话框中分别开启。
				</p>
			{:else}
				<p class="mb-3 text-[13px] text-mist-400">站点所有者已关闭本组织的公开页面。</p>
			{/if}
			<label class="block"
				><span class="field-label">公开页面显示的 Discord 邀请链接</span>
				<span class="join w-full">
					<input
						class="input font-mono text-[12.5px]"
						type="url"
						bind:value={inviteUrl}
						placeholder="https://discord.gg/…"
						maxlength="200"
					/>
					<button
						type="button"
						class="btn btn-sm h-auto"
						onclick={saveInvite}
						disabled={busy || inviteUrl.trim() === data.org.discordInviteUrl}>保存</button
					>
				</span>
			</label>
			<p class="note">填写 discord.gg 或 discord.com/invite 邀请链接；留空会移除按钮。</p>
		</div>

		<div class="panel">
			<div class="mb-3 flex items-center gap-3">
				<span class="label-sm mb-0!">Discord 网络钩子</span>
				<button class="ml-auto btn btn-sm btn-primary" onclick={() => openWebhook(null)}
					>新建 Webhook</button
				>
			</div>
			<p class="mb-3 text-[13px] text-mist-400">
				每个 Webhook 对应一个 Discord
				频道，并发送你勾选的内容：审计记录（封禁、踢出、规则操作、登录）、游戏击杀事件中的误杀队友，以及每台服务器的实时状态卡片（地图和在线玩家）。一个频道添加一个
				Webhook；只勾选“误杀队友”即可创建专用频道。在 Discord 频道设置中打开“集成 →
				Webhook”，复制地址后粘贴到这里。
			</p>
			{#each data.webhooks as w (w.id)}
				<div class="kv items-start">
					<div class="min-w-0">
						<div>
							{w.label}
							{#if !w.enabled}<Badge class="ml-1">已暂停</Badge>{/if}
							{#if w.lastError}<Badge tone="err" class="ml-1">异常</Badge
								>{:else if w.lastSentAt}<Badge tone="ok" class="ml-1">ok</Badge>{/if}
						</div>
						<div class="truncate font-mono text-[11px] text-mist-600">{w.urlHint}</div>
						<div class="text-[12px] text-mist-400">
							{[
								...(w.statusEnabled ? [`Status cards (${w.statusStyle})`] : []),
								...w.events.map(eventLabel)
							].join(' · ')}
							{#if w.serverIds}· {w.serverIds.length} 服务器{w.serverIds.length === 1
									? ''
									: 's'}{/if}
							{#if w.lastError}<div class="text-danger">{w.lastError}</div>{:else if w.lastSentAt}·
								上次发送 {fmtTime(w.lastSentAt)}{/if}
						</div>
					</div>
					<span class="inline-flex shrink-0 flex-wrap justify-end gap-1.5">
						<button class="btn btn-sm" onclick={() => testWebhook(w)} disabled={busy}>测试</button>
						{#if w.statusEnabled}
							<button class="btn btn-sm" onclick={() => testCard(w)} disabled={busy || !w.enabled}
								>测试卡片</button
							>
						{/if}
						<button class="btn btn-sm" onclick={() => openWebhook(w)}>编辑</button>
						<button class="btn btn-sm" onclick={() => toggleWebhook(w)} disabled={busy}
							>{w.enabled ? 'Pause' : 'Enable'}</button
						>
						<button class="btn btn-sm btn-danger" onclick={() => deleteWebhook(w)} disabled={busy}
							>移除</button
						>
					</span>
				</div>
			{:else}
				<p class="text-[13px] text-mist-600">暂无 Webhook。</p>
			{/each}
		</div>

		<div class="panel">
			<div class="mb-3 flex items-center gap-3">
				<span class="label-sm mb-0!">API 密钥</span>
				<button class="ml-auto btn btn-sm btn-primary" onclick={openKey}>新密钥</button>
			</div>
			<p class="mb-3 text-[13px] text-mist-400">
				供 Discord 机器人或脚本使用的 JSON API
				令牌，可单独指定权限和服务器，但不能管理组织。请求格式见 README。
			</p>
			{#each data.keys as k (k.id)}
				<div class="kv items-start">
					<div class="min-w-0">
						<div>
							{k.label}
							{#if k.revokedAt}<Badge tone="err" class="ml-1">已撤销</Badge
								>{:else if k.expiresAt && new Date(k.expiresAt) < new Date()}<Badge
									tone="err"
									class="ml-1">已过期</Badge
								>{/if}
						</div>
						<div class="truncate font-mono text-[11px] text-mist-600">{k.hint}</div>
						<div class="text-[12px] text-mist-400">
							{capabilitySummary(k.capabilities)} · {keyServers(k)}
							{#if k.lastUsedAt}· 上次使用 {fmtTime(k.lastUsedAt)}{:else}· 从未使用{/if}
							{#if k.expiresAt && !k.revokedAt}· 到期时间 {fmtTime(k.expiresAt)}{/if}
						</div>
					</div>
					{#if !k.revokedAt}
						<button
							class="btn btn-sm shrink-0 btn-danger"
							onclick={() => revokeKey(k)}
							disabled={busy}>撤销</button
						>
					{/if}
				</div>
			{:else}
				<p class="text-[13px] text-mist-600">暂无密钥。</p>
			{/each}
		</div>

		<div class="panel">
			<span class="label-sm">封禁名单与预留位</span>
			<div class="space-y-1.5">
				{#each data.lists.lists as l (l.id)}
					<div class="kv items-center">
						<a
							href="/orgs/{encodeURIComponent(data.org.id)}/{l.kind === 'ban'
								? 'bans'
								: 'reserved'}"
							class="text-accent hover:underline">{l.kind === 'ban' ? '封禁列表' : '预留席位'}</a
						>
						<span class="text-mist-400"
							>{l.entryCount} 条记录{l.entryCount === 1
								? 'y'
								: 'ies'}{#if l.kind === 'reserve' && data.lists.membersReserved}
								· 成员获得席位{/if}</span
						>
					</div>
				{/each}
			</div>
			<p class="note">
				已同步至以下组织的所有服务器： {data.org.name} （见“服务器”标签页）。服务器管理员也可以添加和移除名单条目。
			</p>
		</div>
	</div>
</div>

{#if dialog?.kind === 'invite'}
	{@const d = dialog}
	<Modal title="新邀请链接" onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				createInvite();
			}}
		>
			<label class="block"
				><span class="field-label">标签（仅供管理员查看）</span><input
					class="input"
					type="text"
					bind:value={d.label}
					placeholder="例如 #recruitment"
					maxlength="60"
				/></label
			>
			<div class="grid grid-cols-2 gap-2">
				<label class="block"
					><span class="field-label">加入时的角色</span>
					<select class="input" bind:value={d.orgRole}
						><option value="member">成员</option><option value="owner">所有者</option></select
					>
				</label>
				<label class="block"
					><span class="field-label">当前服务器的访问权限</span>
					<select class="input" bind:value={d.serverRoleId}>
						<option value="">无（稍后授予）</option>
						{#each data.roles as r (r.id)}<option value={r.id}>{r.name}</option>{/each}
					</select>
				</label>
				<label class="block"
					><span class="field-label">到期时间</span>
					<select class="input" bind:value={d.expiresDays}>
						<option value="1">1 天后</option>
						<option value="7">7 天后</option>
						<option value="30">30 天后</option>
						<option value="">从不</option>
					</select>
				</label>
				<label class="block"
					><span class="field-label">最多使用次数</span><input
						class="input"
						type="number"
						bind:value={d.maxUses}
						placeholder="不限"
						min="1"
					/></label
				>
			</div>
			<p class="note">
				An <b>所有者</b> 邀请链接可让加入者管理组织并操作所有服务器。请设置较短有效期和一次使用限制。各服务器角色的权限在“角色”标签页设置。
			</p>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button type="submit" class="btn btn-primary" disabled={busy}>创建链接</button>
			</div>
		</form>
	</Modal>
{:else if dialog?.kind === 'created'}
	{@const d = dialog}
	<Modal title="邀请链接已创建" onclose={() => (dialog = null)}>
		<p class="mb-3 text-[13.5px]">复制到 Discord 中：</p>
		<div class="join w-full">
			<input class="input font-mono text-[12.5px]" type="text" readonly value={d.invite.url} />
			<button type="button" class="btn btn-primary" onclick={() => copy(d.invite.url)}>复制</button>
		</div>
		<p class="note">
			加入时的角色 <b>{d.invite.orgRole}</b>{#if d.invite.serverRoleName}, <b
					>{d.invite.serverRoleName}</b
				> 在当前所有服务器上{/if}. {d.invite.expiresAt
				? `Expires ${fmtTime(d.invite.expiresAt)}.`
				: '永不过期。'}
			{d.invite.maxUses ? `最多使用 ${d.invite.maxUses} 次。` : ''}
		</p>
		{#snippet actions()}<button type="button" class="btn" onclick={() => (dialog = null)}
				>完成</button
			>{/snippet}
	</Modal>
{:else if dialog?.kind === 'webhook'}
	{@const d = dialog}
	<Modal title={d.id ? '编辑网络钩子' : '新建 Discord 网络钩子'} onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				saveWebhook();
			}}
		>
			<label class="block"
				><span class="field-label">标签</span><input
					class="input"
					type="text"
					bind:value={d.label}
					placeholder="例如 #admin-log"
					maxlength="60"
				/></label
			>
			<label class="block"
				><span class="field-label">网络钩子地址{d.id ? '（留空则保持不变）' : ''}</span><input
					class="input font-mono text-[12.5px]"
					type="url"
					bind:value={d.url}
					placeholder="https://discord.com/api/webhooks/…"
					required={!d.id}
					autocomplete="off"
				/></label
			>
			<div>
				<span class="field-label">实时状态</span>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={d.status} /> 在频道中保留状态卡片</label
				>
				{#if d.status}
					<label class="mt-2 block"
						><span class="field-label">卡片样式</span><select class="input" bind:value={d.style}>
							{#each STATUS_STYLES as st (st)}<option value={st}>{STATUS_STYLE_LABELS[st]}</option
								>{/each}
						</select></label
					>
					<div class="mt-2 space-y-3">
						<CardOptions
							bind:interval={d.interval}
							bind:linkStatus={d.linkStatus}
							bind:linkLeaderboard={d.linkLeaderboard}
							bind:linkPanel={d.linkPanel}
						/>
					</div>
				{/if}
				<p class="note mt-1">
					后台进程会为每台服务器创建一张状态卡片并持续更新：在线人数、地图、各阵营分数和玩家名单。可以在
					Discord 中置顶。暂停 Webhook 或关闭此功能后，卡片会被移除。{#if d.status && !data.https}
						<b> 面板未使用 HTTPS，卡片不会附带地图图片或图标。</b>{/if}
				</p>
			</div>
			<div>
				<span class="field-label">同步镜像</span>
				<div class="space-y-1">
					{#each data.webhookEvents as e (e.key)}
						<label class="flex items-center gap-2 text-[13px]"
							><input type="checkbox" bind:checked={d.events[e.key]} /> {e.label}</label
						>
					{/each}
				</div>
			</div>
			{#if data.orgServers.length > 1}
				<div>
					<span class="field-label">服务器</span>
					<label class="flex items-center gap-2 text-[13px]"
						><input type="checkbox" bind:checked={d.allServers} /> 组织中的每台服务器</label
					>
					{#if !d.allServers}
						<div class="mt-1 space-y-1 pl-5">
							{#each data.orgServers as s (s.id)}
								<label class="flex items-center gap-2 text-[13px]"
									><input type="checkbox" bind:checked={d.servers[s.id]} /> {s.name}</label
								>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
			<p class="note">
				持有此地址的人都能向该频道发消息，因此地址会加密保存，之后不会再次显示。不会向 Discord 发送
				IP 地址。
			</p>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button
					type="submit"
					class="btn btn-primary"
					disabled={busy || (!d.allServers && !Object.values(d.servers).some(Boolean))}
					>{d.id ? '保存' : '添加网络钩子'}</button
				>
			</div>
		</form>
	</Modal>
{:else if dialog?.kind === 'key'}
	{@const d = dialog}
	<Modal title="新 API 密钥" onclose={() => (dialog = null)}>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				createKey();
			}}
		>
			<label class="block"
				><span class="field-label">标签</span><input
					class="input"
					type="text"
					bind:value={d.label}
					placeholder="例如 Discord 机器人"
					maxlength="60"
					required
				/></label
			>
			<div>
				<span class="field-label">可能</span>
				<CapabilityPicker bind:value={d.capabilities} compact />
			</div>
			<div>
				<span class="field-label">服务器</span>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={d.allServers} /> 包括以后新增的组织服务器</label
				>
				{#if !d.allServers}
					<div class="mt-1 space-y-1 pl-5">
						{#each data.orgServers as s (s.id)}
							<label class="flex items-center gap-2 text-[13px]"
								><input type="checkbox" bind:checked={d.servers[s.id]} /> {s.name}</label
							>
						{/each}
					</div>
				{/if}
			</div>
			<label class="block"
				><span class="field-label">到期时间</span>
				<select class="input" bind:value={d.expiresDays}>
					<option value="">从不</option>
					<option value="30">30 天后</option>
					<option value="90">90 天后</option>
					<option value="365">一年后</option>
				</select>
			</label>
			<p class="note">
				组织封禁名单需要 <b>组织封禁名单</b> ，组织预留位需要
				<b>组织预留位</b>；要读取服务器数据还需要 <b>查看</b>。令牌只显示一次，并以摘要形式保存。
			</p>
			<div class="flex justify-end gap-2 pt-2">
				<button type="button" class="btn" data-close onclick={() => (dialog = null)}>取消</button>
				<button
					type="submit"
					class="btn btn-primary"
					disabled={busy ||
						!d.capabilities.length ||
						(!d.allServers && !Object.values(d.servers).some(Boolean))}>创建密钥</button
				>
			</div>
		</form>
	</Modal>
{:else if dialog?.kind === 'keyCreated'}
	{@const d = dialog}
	<Modal title="API 密钥已创建" onclose={() => (dialog = null)}>
		<p class="mb-3 text-[13.5px]">
			请立即复制令牌；关闭后无法再次查看。发送请求时使用
			<code class="font-mono text-[12.5px]">Authorization: Bearer …</code> 标头调用
			<code class="font-mono text-[12.5px]">/api</code> 接口。
		</p>
		<div class="join w-full">
			<input class="input font-mono text-[12.5px]" type="text" readonly value={d.token} />
			<button type="button" class="btn btn-primary" onclick={() => copy(d.token, 'API 密钥')}
				>复制</button
			>
		</div>
		<p class="note">
			<b>{d.key.label}</b>: {capabilitySummary(d.key.capabilities)} · {keyServers(d.key)}.
			{d.key.expiresAt ? `Expires ${fmtTime(d.key.expiresAt)}.` : '永不过期。'}
		</p>
		{#snippet actions()}<button type="button" class="btn" onclick={() => (dialog = null)}
				>完成</button
			>{/snippet}
	</Modal>
{/if}
