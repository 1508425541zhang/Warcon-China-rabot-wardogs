<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import { api, errorMessage } from '$lib/api';
	import { addPasskey, passkeysSupported, suggestPasskeyName } from '$lib/passkeys';
	import Badge from '$lib/components/Badge.svelte';
	import RoleBadge from '$lib/components/RoleBadge.svelte';
	import DiscordMark from '$lib/components/DiscordMark.svelte';
	import SteamMark from '$lib/components/SteamMark.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let forced = $derived(!!page.url.searchParams.get('force') || data.user.mustChangePassword);
	let recovered = $derived(page.url.searchParams.get('recovered') === '1');
	let busy = $state(false);

	// Passkeys talk to /api/passkeys directly (a WebAuthn ceremony cannot go through a form post).
	let canPasskey = $state(false);
	let passkeyName = $state('');
	$effect(() => {
		canPasskey = passkeysSupported();
		passkeyName = suggestPasskeyName();
	});
	async function newPasskey() {
		busy = true;
		try {
			await addPasskey(passkeyName.trim() || 'Passkey');
			toast('通行密钥已添加。', 'ok');
			passkeyName = suggestPasskeyName();
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function removePasskey(id: string, name: string) {
		if (
			!(await confirmDialog(`Remove the passkey "${name}"?`, { okLabel: 'Remove', danger: true }))
		)
			return;
		busy = true;
		try {
			await api('DELETE', `/api/passkeys/${encodeURIComponent(id)}`);
			toast('通行密钥已移除。', 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	// Secrets that are shown once (backup codes, recovery key) stay on screen until dismissed.
	let backupCodes = $state<string[] | null>(null);
	let recoveryKey = $state<string | null>(null);
	let totp = $state<{ svg: string; secret: string; uri: string } | null>(null);
	let steamLinked = $derived(data.providers.includes('steam'));
	let discordLinked = $derived(data.providers.includes('discord'));
	let wantRemovePassword = $state(false);

	async function copy(text: string, what: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast(`${what} copied.`, 'ok');
		} catch {
			toast('复制失败，请手动选中文本。', 'err');
		}
	}

	$effect(() => {
		if (form?.changed) toast('密码已修改，其他会话已退出登录。', 'ok');
		if (form?.set) toast(`密码已设置，现在也可使用 @${data.user.username} 登录。`, 'ok');
		if (form?.passwordRemoved) toast('密码已移除。', 'ok');
		if (form?.revoked) toast('会话已撤销。', 'ok');
		if (form?.unlinked)
			toast(`${form.unlinked === 'steam' ? 'Steam' : 'Discord'} 账号已解除关联。`, 'ok');
		if (form?.steam) toast(form.steamId ? 'SteamID 已保存。' : 'SteamID 已移除。', 'ok');
		if (form?.enabled) toast('验证器应用已绑定。', 'ok');
		if (form?.disabled) toast('验证器应用已移除。', 'ok');
		if (form?.recoveryKeyCleared) toast('恢复密钥已废弃。', 'ok');
		if (form?.totp) totp = form.totp;
		if (form?.enabled) totp = null;
		if (form?.backupCodes) backupCodes = form.backupCodes;
		if (form?.recoveryKey) recoveryKey = form.recoveryKey;
		if (form?.defaultOrg)
			toast(
				form.orgName ? `面板现在默认打开“${form.orgName}”。` : '面板现在可访问所有组织。',
				'ok'
			);
		if (form?.error) toast(form.error, 'err');
	});
	$effect(() => {
		const linked = page.url.searchParams.get('linked');
		if (linked === 'steam') toast('Steam 账号已关联，现在可以使用它登录。', 'ok');
		if (linked === 'discord') toast('Discord 账号已关联。', 'ok');
		const err = page.url.searchParams.get('error');
		if (err === 'steam_taken') toast('此 Steam 账号已关联其他用户。', 'err');
		else if (err?.startsWith('steam')) toast('Steam 未确认关联，请重试。', 'err');
	});
</script>

<svelte:head><title>账号 · {data.appName}</title></svelte:head>

<h1 class="mb-5 text-xl font-semibold tracking-tight">账号</h1>

{#if forced}
	<div class="callout">使用面板前必须设置新密码。</div>
{/if}
{#if recovered}
	<div class="callout">
		你刚使用恢复密钥登录，该密钥现已失效。离开此页前请重新设置登录方式：至少保留两种，并在需要时生成新的恢复密钥。
	</div>
{/if}

<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
	<div class="panel">
		<div class="mb-2 flex flex-wrap items-center justify-between gap-2">
			<span class="label-sm !mb-0">登录方式</span>
			{#if data.enrolment.complete}
				<Badge tone="ok">符合要求</Badge>
			{:else if !data.policy.enforced}
				<Badge tone="warn">推荐</Badge>
			{:else if data.status.due}
				<Badge tone="err">需要处理</Badge>
			{:else if data.status.daysLeft !== null}
				<Badge tone="warn">剩余 {data.status.daysLeft} 天</Badge>
			{:else}
				<Badge tone="warn">未完成</Badge>
			{/if}
		</div>
		<div class="kv">
			<span class="text-mist-400">当前登录账号</span>
			<span
				>{data.user.name} <span class="font-mono text-mist-400">@{data.user.username}</span>
				<RoleBadge role={data.user.role} /></span
			>
		</div>
		{#if data.enrolment.complete}
			<p class="mt-2 text-[13px] text-mist-400">
				已设置 {data.enrolment.waysIn} 种独立登录方式。丢失一台设备或一个关联账号后仍可登录。
			</p>
		{:else}
			<div class="mt-3 rounded-ctl border border-warn/30 bg-warn/10 px-3 py-2 text-[13px]">
				<div class="mb-1 font-medium">待办事项</div>
				<ul class="list-disc space-y-1 pl-4 text-mist-200">
					{#each data.enrolment.problems as p (p)}<li>{p}</li>{/each}
				</ul>
				{#if !data.policy.enforced}
					<p class="mt-2 text-mist-400">
						{#if data.policy.nudge}
							当前不会强制执行登录方式要求。建议补充第二种方式，以便设备丢失或密码泄露时恢复账号。
						{:else}
							你的角色无需强制设置，但增加备用登录方式可以避免丢失账号。
						{/if}
					</p>
				{:else if data.status.due}
					<p class="mt-2 text-mist-400">完成此步骤前无法使用面板其他功能。</p>
				{:else if data.status.deadline}
					<p class="mt-2 text-mist-400">
						你可以继续使用面板至 {fmtTime(data.status.deadline)}。
					</p>
				{/if}
			</div>
		{/if}

		<!-- Passkeys -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">通行密钥</span>
			{#if data.passkeys.length}
				<ul class="mb-3 divide-y divide-white/8">
					{#each data.passkeys as p (p.id)}
						<li class="flex items-center justify-between gap-3 py-2 text-[13px]">
							<div>
								<div>{p.name}</div>
								<div class="text-[12px] text-mist-500">
									添加于 {fmtTime(p.createdAt)}{p.backedUp ? ' · 已同步' : ' · 仅此设备'}
								</div>
							</div>
							<button
								class="btn btn-sm btn-danger"
								type="button"
								disabled={busy}
								onclick={() => removePasskey(p.id, p.name)}>移除</button
							>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="mb-3 text-[13px] text-mist-400">
					尚未添加通行密钥。它使用设备的 Face ID、指纹、Windows Hello
					或安全密钥验证，能抵御钓鱼，并可独立满足双重验证要求。
				</p>
			{/if}
			{#if canPasskey}
				<div class="join w-full">
					<input
						class="input"
						type="text"
						maxlength="60"
						placeholder="名称，例如 iPhone"
						bind:value={passkeyName}
					/>
					<button class="btn btn-primary" type="button" disabled={busy} onclick={newPasskey}
						>添加通行密钥</button
					>
				</div>
				<p class="note">建议在常用设备上分别添加通行密钥，避免丢失手机后无法登录。</p>
			{:else}
				<p class="note">当前浏览器不支持通行密钥。</p>
			{/if}
		</div>

		<!-- Authenticator app -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">身份验证器</span>
			{#if data.methods.twoFactor}
				<div class="flex flex-wrap items-center gap-3">
					<Badge tone="ok">开启</Badge>
					<span class="text-[13px] text-mist-400">使用密码登录时需输入六位验证码。</span>
				</div>
				<div class="mt-3 grid gap-3 sm:grid-cols-2">
					<form method="post" action="?/backupCodes" use:enhance class="space-y-2">
						{#if data.hasPassword}
							<input
								class="input"
								type="password"
								name="password"
								autocomplete="current-password"
								placeholder="你的密码"
								required
							/>
						{/if}
						<button class="btn w-full" type="submit" disabled={busy}>新备用码</button>
					</form>
					<form
						method="post"
						action="?/totpDisable"
						class="space-y-2"
						use:enhance={async ({ cancel }) => {
							if (
								!(await confirmDialog(
									'关闭身份验证器？之后只凭密码即可登录，这不符合当前登录方式要求。',
									{ okLabel: '关闭', danger: true }
								))
							)
								cancel();
						}}
					>
						{#if data.hasPassword}
							<input
								class="input"
								type="password"
								name="password"
								autocomplete="current-password"
								placeholder="你的密码"
								required
							/>
						{/if}
						<button class="btn w-full btn-danger" type="submit" disabled={busy}>关闭</button>
					</form>
				</div>
			{:else if totp}
				<p class="mb-3 text-[13px] text-mist-400">
					使用身份验证器（如 1Password、Bitwarden、Google Authenticator 或
					Authy）扫描二维码，再输入显示的验证码。
				</p>
				<div class="flex flex-wrap items-start gap-4">
					<div class="rounded-ctl bg-white p-2">{@html totp.svg}</div>
					<div class="min-w-0 flex-1 space-y-2 text-[13px]">
						<div class="text-mist-400">或输入密钥：</div>
						<div class="flex items-center gap-2">
							<code
								class="min-w-0 flex-1 truncate rounded-ctl bg-ink-950 px-2 py-1 font-mono text-[12px]"
								>{totp.secret}</code
							>
							<button class="btn btn-sm" type="button" onclick={() => copy(totp!.secret, 'Secret')}
								>复制</button
							>
						</div>
						<form method="post" action="?/totpConfirm" use:enhance class="join w-full pt-2">
							<input
								class="input font-mono tracking-[0.25em]"
								type="text"
								name="code"
								inputmode="numeric"
								autocomplete="one-time-code"
								placeholder="123456"
								required
							/>
							<button class="btn btn-primary" type="submit" disabled={busy}>确认</button>
						</form>
						<button
							type="button"
							class="text-[12.5px] text-mist-400 underline hover:text-mist-100"
							onclick={() => (totp = null)}>取消</button
						>
					</div>
				</div>
			{:else}
				<p class="mb-3 text-[13px] text-mist-400">
					{#if data.hasPassword}
						密码登录的第二重验证：输入手机身份验证器生成的六位验证码。
					{:else}
						当前账号没有密码，因此无需为密码设置第二重验证；以后设置密码时再启用即可。
					{/if}
				</p>
				{#if data.hasPassword}
					<form method="post" action="?/totpStart" use:enhance class="join w-full">
						<input
							class="input"
							type="password"
							name="password"
							autocomplete="current-password"
							placeholder="你的密码"
							required
						/>
						<button class="btn btn-primary" type="submit" disabled={busy}>开启</button>
					</form>
				{/if}
			{/if}
			{#if backupCodes}
				<div class="mt-3 rounded-ctl border border-warn/30 bg-warn/10 p-3 text-[13px]">
					<div class="mb-1 flex items-center justify-between gap-2">
						<span class="font-medium">备用码：请立即妥善保存</span>
						<span class="inline-flex gap-1.5">
							<button
								class="btn btn-sm"
								type="button"
								onclick={() => copy(backupCodes!.join('\n'), '备用码')}>复制</button
							>
							<button class="btn btn-sm" type="button" onclick={() => (backupCodes = null)}
								>完成</button
							>
						</span>
					</div>
					<p class="mb-2 text-mist-400">
						每个备用码可替代身份验证器验证码使用一次，关闭后不会再次显示。
					</p>
					<div class="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-[12.5px] sm:grid-cols-3">
						{#each backupCodes as c (c)}<span>{c}</span>{/each}
					</div>
				</div>
			{/if}
		</div>

		<!-- Linked accounts -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">关联账号</span>
			<p class="mb-3 text-[13px] text-mist-400">
				关联现有账号后可用其登录，也能在设备丢失时恢复访问。
			</p>
			<div class="grid gap-3 sm:grid-cols-2">
				{#if data.discord}
					<div
						class="flex items-center justify-between gap-3 rounded-ctl border border-white/8 px-3 py-2"
					>
						<span class="inline-flex items-center gap-2 text-[13px]"><DiscordMark />Discord</span>
						{#if discordLinked}
							<span class="inline-flex items-center gap-2">
								<Badge tone="ok">已关联</Badge>
								<form method="post" action="?/unlinkDiscord" use:enhance>
									<button class="btn btn-sm" type="submit" disabled={busy}>取消关联</button>
								</form>
							</span>
						{:else}
							<form method="post" action="?/linkDiscord" use:enhance>
								<button class="btn btn-sm" type="submit" disabled={busy}>关联</button>
							</form>
						{/if}
					</div>
				{/if}
				<div
					class="flex items-center justify-between gap-3 rounded-ctl border border-white/8 px-3 py-2"
				>
					<span class="inline-flex items-center gap-2 text-[13px]"><SteamMark />Steam</span>
					{#if steamLinked}
						<span class="inline-flex items-center gap-2">
							<Badge tone="ok">已关联</Badge>
							<form method="post" action="?/unlinkSteam" use:enhance>
								<button class="btn btn-sm" type="submit" disabled={busy}>取消关联</button>
							</form>
						</span>
					{:else}
						<form method="post" action="?/linkSteam" use:enhance>
							<button class="btn btn-sm" type="submit" disabled={busy}>关联</button>
						</form>
					{/if}
				</div>
			</div>
		</div>

		<!-- Recovery key -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">恢复密钥</span>
			<p class="mb-3 text-[13px] text-mist-400">
				恢复密钥为 40
				位字符，可在其他登录方式都不可用时使用一次。面板只保存密钥摘要，请打印或存入密码管理器。所有者须保留恢复密钥或关联账号。
			</p>
			{#if recoveryKey}
				<div class="rounded-ctl border border-warn/30 bg-warn/10 p-3 text-[13px]">
					<div class="mb-1 flex items-center justify-between gap-2">
						<span class="font-medium">你的恢复密钥：请立即妥善保存</span>
						<span class="inline-flex gap-1.5">
							<button
								class="btn btn-sm"
								type="button"
								onclick={() => copy(recoveryKey!, '恢复密钥')}>复制</button
							>
							<button class="btn btn-sm" type="button" onclick={() => (recoveryKey = null)}
								>完成</button
							>
						</span>
					</div>
					<code class="block font-mono text-[13px] break-all">{recoveryKey}</code>
					<p class="mt-2 text-mist-400">密钥不会再次显示，使用后即失效。</p>
				</div>
			{:else}
				<div class="flex flex-wrap items-center gap-3">
					{#if data.methods.recoveryKey}
						<Badge tone="ok">保存于 {fmtTime(data.recoveryKeyAt)}</Badge>
					{:else}
						<Badge tone="warn">无</Badge>
					{/if}
					<form
						method="post"
						action="?/recoveryKey"
						use:enhance={async ({ cancel }) => {
							if (
								data.methods.recoveryKey &&
								!(await confirmDialog('生成新的恢复密钥？当前密钥会立即失效。', {
									okLabel: '生成'
								}))
							)
								cancel();
						}}
					>
						<button class="btn btn-sm" type="submit" disabled={busy}
							>{data.methods.recoveryKey ? '更换密钥' : '生成密钥'}</button
						>
					</form>
					{#if data.methods.recoveryKey}
						<form method="post" action="?/recoveryKeyClear" use:enhance>
							<button class="btn btn-sm" type="submit" disabled={busy}>放弃</button>
						</form>
					{/if}
				</div>
			{/if}
		</div>

		<!-- Password -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">{data.hasPassword ? '密码' : '密码（可选）'}</span>
			{#if !data.hasPassword}
				<p class="mb-2 text-[13px] text-mist-400">
					当前账号没有密码。只有在无法使用通行密钥或关联账号的环境中才需要设置密码；设置后还需启用身份验证器。
				</p>
			{/if}
			<form
				method="post"
				action="?/password"
				class="space-y-3"
				use:enhance={() => {
					busy = true;
					return async ({ update }) => {
						await update({ reset: true });
						busy = false;
					};
				}}
			>
				{#if data.hasPassword}
					<label class="block"
						><span class="field-label">当前密码</span><input
							class="input"
							type="password"
							name="current"
							autocomplete="current-password"
							required
						/></label
					>
				{/if}
				<label class="block"
					><span class="field-label">新密码（至少 10 位）</span><input
						class="input"
						type="password"
						name="next"
						autocomplete="new-password"
						minlength="10"
						required
					/></label
				>
				<label class="block"
					><span class="field-label">再次输入新密码</span><input
						class="input"
						type="password"
						name="again"
						autocomplete="new-password"
						minlength="10"
						required
					/></label
				>
				<button class="btn {data.hasPassword ? 'btn-primary' : ''}" type="submit" disabled={busy}
					>{data.hasPassword ? '修改密码' : '设置密码'}</button
				>
			</form>
			{#if data.hasPassword && (data.passkeys.length || steamLinked || discordLinked)}
				<div class="mt-3">
					{#if wantRemovePassword}
						<form method="post" action="?/removePassword" use:enhance class="join w-full">
							<input
								class="input"
								type="text"
								name="confirm"
								autocomplete="off"
								spellcheck="false"
								placeholder="输入 @{data.user.username} 以确认"
								required
							/>
							<button class="btn btn-danger" type="submit" disabled={busy}>移除密码</button>
						</form>
					{:else}
						<button
							type="button"
							class="text-[12.5px] text-mist-400 underline hover:text-mist-100"
							onclick={() => (wantRemovePassword = true)}
							>移除密码，仅使用通行密钥和关联账号登录</button
						>
					{/if}
				</div>
			{/if}
		</div>
	</div>

	<div class="space-y-4">
		<div class="panel">
			<span class="label-sm">你的会话</span>
			<div class="table-wrap">
				<table>
					<thead><tr><th>开始时间</th><th>最近出现</th><th>客户端</th><th></th></tr></thead>
					<tbody>
						{#each data.sessions as s (s.id)}
							<tr>
								<td class="whitespace-nowrap">{fmtTime(s.createdAt)}</td>
								<td class="whitespace-nowrap">{fmtTime(s.updatedAt)}</td>
								<td class="max-w-[160px] truncate text-mist-400" title={s.userAgent}
									>{s.userAgent.replace(/^Mozilla\/5\.0 /, '').slice(0, 28)}</td
								>
								<td class="text-right">
									{#if s.current}
										<Badge tone="ok">当前会话</Badge>
									{:else}
										<form method="post" action="?/revoke" use:enhance>
											<input type="hidden" name="id" value={s.id} />
											<button class="btn btn-sm btn-danger" type="submit">撤销</button>
										</form>
									{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>

		<div class="panel">
			<span class="label-sm">预留位使用的 SteamID</span>
			<form method="post" action="?/steam" use:enhance class="join w-full">
				<input
					class="input font-mono"
					type="text"
					name="steamId"
					inputmode="numeric"
					maxlength="17"
					placeholder="SteamID64，例如 7656119…"
					value={data.steamId}
				/>
				<button class="btn" type="submit">保存</button>
			</form>
			<p class="note">
				填写你自己的 SteamID64；关联上方 Steam
				账号后会自动填入。组织据此向成员分配预留位，留空表示不参与。
			</p>

			{#if data.orgs.length > 1}
				<div class="mt-6 border-t border-white/8 pt-4">
					<span class="label-sm">默认组织</span>
					<form method="post" action="?/defaultOrg" use:enhance class="join w-full">
						<select class="input" name="orgId" value={data.defaultOrgId}>
							<option value="">每个组织</option>
							{#each data.orgs as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
						</select>
						<button class="btn" type="submit">保存</button>
					</form>
					<p class="note">
						总览、服务器切换器和服务器页面默认只显示该组织；顶部选择器可在当前浏览器中临时切换。
					</p>
				</div>
			{/if}
		</div>
	</div>

	<div class="panel lg:col-span-2">
		<span class="label-sm">删除账号</span>
		<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
			<div class="space-y-2 text-[13px] leading-relaxed text-mist-400">
				<p>删除账号会立即移除登录凭据、通行密钥、会话、服务器角色和组织成员身份，无法撤销。</p>
				<p>
					你产生的审计记录会保留，但会去除姓名、IP
					地址和浏览器信息。你创建的组织与服务器仍归其他所有者管理。若你是某组织或站点的唯一所有者，则无法删除账号。
				</p>
			</div>
			<form
				method="post"
				action="?/deleteAccount"
				class="space-y-3"
				use:enhance={async ({ cancel }) => {
					const ok = await confirmDialog('删除账号及全部登录凭据？此操作无法撤销。', {
						title: '删除账号',
						okLabel: '删除我的账号',
						danger: true
					});
					if (!ok) {
						cancel();
						return;
					}
					busy = true;
					return async ({ update }) => {
						await update();
						busy = false;
					};
				}}
			>
				{#if data.hasPassword}
					<label class="block"
						><span class="field-label">你的密码</span><input
							class="input"
							type="password"
							name="password"
							autocomplete="current-password"
							required
						/></label
					>
				{:else}
					<label class="block"
						><span class="field-label">输入用户名（@{data.user.username}）以确认</span><input
							class="input"
							type="text"
							name="confirm"
							autocomplete="off"
							spellcheck="false"
							required
						/></label
					>
				{/if}
				<button class="btn btn-danger" type="submit" disabled={busy}>删除我的账号</button>
			</form>
		</div>
	</div>
</div>
