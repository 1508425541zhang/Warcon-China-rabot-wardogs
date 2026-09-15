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
			toast('Passkey added.', 'ok');
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
			toast('Passkey removed.', 'ok');
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
			toast('Could not copy; select the text instead.', 'err');
		}
	}

	$effect(() => {
		if (form?.changed) toast('Password changed. Other sessions were signed out.', 'ok');
		if (form?.set) toast(`Password set. You can now also sign in as @${data.user.username}.`, 'ok');
		if (form?.passwordRemoved) toast('Password removed.', 'ok');
		if (form?.revoked) toast('Session revoked.', 'ok');
		if (form?.unlinked) toast(`${form.unlinked === 'steam' ? 'Steam' : 'Discord'} unlinked.`, 'ok');
		if (form?.steam) toast(form.steamId ? 'SteamID saved.' : 'SteamID removed.', 'ok');
		if (form?.enabled) toast('Authenticator app enrolled.', 'ok');
		if (form?.disabled) toast('Authenticator app removed.', 'ok');
		if (form?.recoveryKeyCleared) toast('Recovery key discarded.', 'ok');
		if (form?.totp) totp = form.totp;
		if (form?.enabled) totp = null;
		if (form?.backupCodes) backupCodes = form.backupCodes;
		if (form?.recoveryKey) recoveryKey = form.recoveryKey;
		if (form?.defaultOrg)
			toast(
				form.orgName
					? `The panel now opens on ${form.orgName}.`
					: 'The panel now opens on every organisation.',
				'ok'
			);
		if (form?.error) toast(form.error, 'err');
	});
	$effect(() => {
		const linked = page.url.searchParams.get('linked');
		if (linked === 'steam') toast('Steam linked. You can sign in with it from now on.', 'ok');
		if (linked === 'discord') toast('Discord linked.', 'ok');
		const err = page.url.searchParams.get('error');
		if (err === 'steam_taken')
			toast('That Steam account is already linked to another user.', 'err');
		else if (err?.startsWith('steam')) toast('Steam did not confirm the link. Try again.', 'err');
	});
</script>

<svelte:head><title>Account · {data.appName}</title></svelte:head>

<h1 class="mb-5 text-xl font-semibold tracking-tight">Account</h1>

{#if forced}
	<div class="callout">You must set a new password before using the panel.</div>
{/if}
{#if recovered}
	<div class="callout">
		You signed in with your recovery key, which is now used up. Set up your sign-in methods again
		below before you leave: at least two ways in, and a new recovery key if you rely on one.
	</div>
{/if}

<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
	<div class="panel">
		<div class="mb-2 flex flex-wrap items-center justify-between gap-2">
			<span class="label-sm !mb-0">Sign-in methods</span>
			{#if data.enrolment.complete}
				<Badge tone="ok">meets the rules</Badge>
			{:else if !data.policy.enforced}
				<Badge tone="warn">recommended</Badge>
			{:else if data.status.due}
				<Badge tone="err">action required</Badge>
			{:else if data.status.daysLeft !== null}
				<Badge tone="warn"
					>{data.status.daysLeft} day{data.status.daysLeft === 1 ? '' : 's'} left</Badge
				>
			{:else}
				<Badge tone="warn">incomplete</Badge>
			{/if}
		</div>
		<div class="kv">
			<span class="text-mist-400">Signed in as</span>
			<span
				>{data.user.name} <span class="font-mono text-mist-400">@{data.user.username}</span>
				<RoleBadge role={data.user.role} /></span
			>
		</div>
		{#if data.enrolment.complete}
			<p class="mt-2 text-[13px] text-mist-400">
				{data.enrolment.waysIn} independent ways in. Losing one device or provider will not lock you out.
			</p>
		{:else}
			<div class="mt-3 rounded-ctl border border-warn/30 bg-warn/10 px-3 py-2 text-[13px]">
				<div class="mb-1 font-medium">To do</div>
				<ul class="list-disc space-y-1 pl-4 text-mist-200">
					{#each data.enrolment.problems as p (p)}<li>{p}</li>{/each}
				</ul>
				{#if !data.policy.enforced}
					<p class="mt-2 text-mist-400">
						{#if data.policy.nudge}
							This panel does not enforce the rules, but a lost device or a phished password is
							still a lost account. Two ways in takes a minute.
						{:else}
							Not required for your role: you hold no server powers worth stealing. Still worth a
							minute if you value the account.
						{/if}
					</p>
				{:else if data.status.due}
					<p class="mt-2 text-mist-400">The rest of the panel is closed until this is done.</p>
				{:else if data.status.deadline}
					<p class="mt-2 text-mist-400">
						The panel keeps working until {fmtTime(data.status.deadline)}.
					</p>
				{/if}
			</div>
		{/if}

		<!-- Passkeys -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">Passkeys</span>
			{#if data.passkeys.length}
				<ul class="mb-3 divide-y divide-white/8">
					{#each data.passkeys as p (p.id)}
						<li class="flex items-center justify-between gap-3 py-2 text-[13px]">
							<div>
								<div>{p.name}</div>
								<div class="text-[12px] text-mist-500">
									added {fmtTime(p.createdAt)}{p.backedUp ? ' · synced' : ' · this device only'}
								</div>
							</div>
							<button
								class="btn btn-sm btn-danger"
								type="button"
								disabled={busy}
								onclick={() => removePasskey(p.id, p.name)}>Remove</button
							>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="mb-3 text-[13px] text-mist-400">
					No passkeys yet. A passkey is your device's own lock (Face ID, fingerprint, Windows Hello
					or a security key); it is phishing-proof and counts as two factors on its own.
				</p>
			{/if}
			{#if canPasskey}
				<div class="join w-full">
					<input
						class="input"
						type="text"
						maxlength="60"
						placeholder="Name, e.g. iPhone"
						bind:value={passkeyName}
					/>
					<button class="btn btn-primary" type="button" disabled={busy} onclick={newPasskey}
						>Add passkey</button
					>
				</div>
				<p class="note">
					Add one per device you sign in from, so losing a phone is not losing the account.
				</p>
			{:else}
				<p class="note">This browser does not support passkeys.</p>
			{/if}
		</div>

		<!-- Authenticator app -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">Authenticator app</span>
			{#if data.methods.twoFactor}
				<div class="flex flex-wrap items-center gap-3">
					<Badge tone="ok">on</Badge>
					<span class="text-[13px] text-mist-400">Password sign-ins ask for a six-digit code.</span>
				</div>
				<div class="mt-3 grid gap-3 sm:grid-cols-2">
					<form method="post" action="?/backupCodes" use:enhance class="space-y-2">
						{#if data.hasPassword}
							<input
								class="input"
								type="password"
								name="password"
								autocomplete="current-password"
								placeholder="Your password"
								required
							/>
						{/if}
						<button class="btn w-full" type="submit" disabled={busy}>New backup codes</button>
					</form>
					<form
						method="post"
						action="?/totpDisable"
						class="space-y-2"
						use:enhance={async ({ cancel }) => {
							if (
								!(await confirmDialog(
									'Turn the authenticator app off? Your password alone will then sign you in, which the sign-in rules do not accept.',
									{ okLabel: 'Turn off', danger: true }
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
								placeholder="Your password"
								required
							/>
						{/if}
						<button class="btn w-full btn-danger" type="submit" disabled={busy}>Turn off</button>
					</form>
				</div>
			{:else if totp}
				<p class="mb-3 text-[13px] text-mist-400">
					Scan this with your authenticator app (1Password, Bitwarden, Google Authenticator,
					Authy…), then enter the code it shows to finish.
				</p>
				<div class="flex flex-wrap items-start gap-4">
					<div class="rounded-ctl bg-white p-2">{@html totp.svg}</div>
					<div class="min-w-0 flex-1 space-y-2 text-[13px]">
						<div class="text-mist-400">Or type the secret:</div>
						<div class="flex items-center gap-2">
							<code
								class="min-w-0 flex-1 truncate rounded-ctl bg-ink-950 px-2 py-1 font-mono text-[12px]"
								>{totp.secret}</code
							>
							<button class="btn btn-sm" type="button" onclick={() => copy(totp!.secret, 'Secret')}
								>Copy</button
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
							<button class="btn btn-primary" type="submit" disabled={busy}>Confirm</button>
						</form>
						<button
							type="button"
							class="text-[12.5px] text-mist-400 underline hover:text-mist-100"
							onclick={() => (totp = null)}>Cancel</button
						>
					</div>
				</div>
			{:else}
				<p class="mb-3 text-[13px] text-mist-400">
					{#if data.hasPassword}
						A second factor for your password: a six-digit code from an app on your phone.
					{:else}
						You have no password, so nothing needs a second factor. Add one only if you set a
						password.
					{/if}
				</p>
				{#if data.hasPassword}
					<form method="post" action="?/totpStart" use:enhance class="join w-full">
						<input
							class="input"
							type="password"
							name="password"
							autocomplete="current-password"
							placeholder="Your password"
							required
						/>
						<button class="btn btn-primary" type="submit" disabled={busy}>Turn on</button>
					</form>
				{/if}
			{/if}
			{#if backupCodes}
				<div class="mt-3 rounded-ctl border border-warn/30 bg-warn/10 p-3 text-[13px]">
					<div class="mb-1 flex items-center justify-between gap-2">
						<span class="font-medium">Backup codes: save these now</span>
						<span class="inline-flex gap-1.5">
							<button
								class="btn btn-sm"
								type="button"
								onclick={() => copy(backupCodes!.join('\n'), 'Backup codes')}>Copy</button
							>
							<button class="btn btn-sm" type="button" onclick={() => (backupCodes = null)}
								>Done</button
							>
						</span>
					</div>
					<p class="mb-2 text-mist-400">
						Each works once in place of an app code. They are not shown again.
					</p>
					<div class="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-[12.5px] sm:grid-cols-3">
						{#each backupCodes as c (c)}<span>{c}</span>{/each}
					</div>
				</div>
			{/if}
		</div>

		<!-- Linked accounts -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">Linked accounts</span>
			<p class="mb-3 text-[13px] text-mist-400">
				Sign in through a provider that already knows you. Each link is a way in, and a way back in
				if a device is lost.
			</p>
			<div class="grid gap-3 sm:grid-cols-2">
				{#if data.discord}
					<div
						class="flex items-center justify-between gap-3 rounded-ctl border border-white/8 px-3 py-2"
					>
						<span class="inline-flex items-center gap-2 text-[13px]"><DiscordMark />Discord</span>
						{#if discordLinked}
							<span class="inline-flex items-center gap-2">
								<Badge tone="ok">linked</Badge>
								<form method="post" action="?/unlinkDiscord" use:enhance>
									<button class="btn btn-sm" type="submit" disabled={busy}>Unlink</button>
								</form>
							</span>
						{:else}
							<form method="post" action="?/linkDiscord" use:enhance>
								<button class="btn btn-sm" type="submit" disabled={busy}>Link</button>
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
							<Badge tone="ok">linked</Badge>
							<form method="post" action="?/unlinkSteam" use:enhance>
								<button class="btn btn-sm" type="submit" disabled={busy}>Unlink</button>
							</form>
						</span>
					{:else}
						<form method="post" action="?/linkSteam" use:enhance>
							<button class="btn btn-sm" type="submit" disabled={busy}>Link</button>
						</form>
					{/if}
				</div>
			</div>
		</div>

		<!-- Recovery key -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">Recovery key</span>
			<p class="mb-3 text-[13px] text-mist-400">
				A 40-character key that signs you in once when everything else is lost. The panel keeps only
				a hash: print it or put it in a password manager. Owners need this or a linked provider.
			</p>
			{#if recoveryKey}
				<div class="rounded-ctl border border-warn/30 bg-warn/10 p-3 text-[13px]">
					<div class="mb-1 flex items-center justify-between gap-2">
						<span class="font-medium">Your recovery key: save it now</span>
						<span class="inline-flex gap-1.5">
							<button
								class="btn btn-sm"
								type="button"
								onclick={() => copy(recoveryKey!, 'Recovery key')}>Copy</button
							>
							<button class="btn btn-sm" type="button" onclick={() => (recoveryKey = null)}
								>Done</button
							>
						</span>
					</div>
					<code class="block font-mono text-[13px] break-all">{recoveryKey}</code>
					<p class="mt-2 text-mist-400">It is not shown again. Using it discards it.</p>
				</div>
			{:else}
				<div class="flex flex-wrap items-center gap-3">
					{#if data.methods.recoveryKey}
						<Badge tone="ok">saved {fmtTime(data.recoveryKeyAt)}</Badge>
					{:else}
						<Badge tone="warn">none</Badge>
					{/if}
					<form
						method="post"
						action="?/recoveryKey"
						use:enhance={async ({ cancel }) => {
							if (
								data.methods.recoveryKey &&
								!(await confirmDialog(
									'Generate a new recovery key? The current one stops working.',
									{
										okLabel: 'Generate'
									}
								))
							)
								cancel();
						}}
					>
						<button class="btn btn-sm" type="submit" disabled={busy}
							>{data.methods.recoveryKey ? 'Replace key' : 'Generate key'}</button
						>
					</form>
					{#if data.methods.recoveryKey}
						<form method="post" action="?/recoveryKeyClear" use:enhance>
							<button class="btn btn-sm" type="submit" disabled={busy}>Discard</button>
						</form>
					{/if}
				</div>
			{/if}
		</div>

		<!-- Password -->
		<div class="mt-6 border-t border-white/8 pt-4">
			<span class="label-sm">{data.hasPassword ? 'Password' : 'Password (optional)'}</span>
			{#if !data.hasPassword}
				<p class="mb-2 text-[13px] text-mist-400">
					This account has no password, which is the recommended state. Set one only if you must
					sign in somewhere passkeys and providers cannot reach; it will then need the authenticator
					app too.
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
						><span class="field-label">Current password</span><input
							class="input"
							type="password"
							name="current"
							autocomplete="current-password"
							required
						/></label
					>
				{/if}
				<label class="block"
					><span class="field-label">New password (10+ characters)</span><input
						class="input"
						type="password"
						name="next"
						autocomplete="new-password"
						minlength="10"
						required
					/></label
				>
				<label class="block"
					><span class="field-label">Repeat new password</span><input
						class="input"
						type="password"
						name="again"
						autocomplete="new-password"
						minlength="10"
						required
					/></label
				>
				<button class="btn {data.hasPassword ? 'btn-primary' : ''}" type="submit" disabled={busy}
					>{data.hasPassword ? 'Change password' : 'Set password'}</button
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
								placeholder="Type @{data.user.username} to confirm"
								required
							/>
							<button class="btn btn-danger" type="submit" disabled={busy}>Remove password</button>
						</form>
					{:else}
						<button
							type="button"
							class="text-[12.5px] text-mist-400 underline hover:text-mist-100"
							onclick={() => (wantRemovePassword = true)}
							>Remove the password and rely on passkeys and linked accounts</button
						>
					{/if}
				</div>
			{/if}
		</div>
	</div>

	<div class="space-y-4">
		<div class="panel">
			<span class="label-sm">Your sessions</span>
			<div class="table-wrap">
				<table>
					<thead
						><tr><th>Started</th><th>Last seen</th><th>IP</th><th>Client</th><th></th></tr></thead
					>
					<tbody>
						{#each data.sessions as s (s.id)}
							<tr>
								<td class="whitespace-nowrap">{fmtTime(s.createdAt)}</td>
								<td class="whitespace-nowrap">{fmtTime(s.updatedAt)}</td>
								<td class="font-mono text-[12px]">{s.ip}</td>
								<td class="max-w-[160px] truncate text-mist-400" title={s.userAgent}
									>{s.userAgent.replace(/^Mozilla\/5\.0 /, '').slice(0, 28)}</td
								>
								<td class="text-right">
									{#if s.current}
										<Badge tone="ok">this session</Badge>
									{:else}
										<form method="post" action="?/revoke" use:enhance>
											<input type="hidden" name="id" value={s.id} />
											<button class="btn btn-sm btn-danger" type="submit">Revoke</button>
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
			<span class="label-sm">Steam ID for reserved slots</span>
			<form method="post" action="?/steam" use:enhance class="join w-full">
				<input
					class="input font-mono"
					type="text"
					name="steamId"
					inputmode="numeric"
					maxlength="17"
					placeholder="SteamID64, e.g. 7656119…"
					value={data.steamId}
				/>
				<button class="btn" type="submit">Save</button>
			</form>
			<p class="note">
				Your own SteamID64 (linking Steam above fills it in). Organisations that hand their members
				a reserved slot use it; leave it blank to opt out.
			</p>

			{#if data.orgs.length > 1}
				<div class="mt-6 border-t border-white/8 pt-4">
					<span class="label-sm">Default organisation</span>
					<form method="post" action="?/defaultOrg" use:enhance class="join w-full">
						<select class="input" name="orgId" value={data.defaultOrgId}>
							<option value="">Every organisation</option>
							{#each data.orgs as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
						</select>
						<button class="btn" type="submit">Save</button>
					</form>
					<p class="note">
						The dashboard, the server switcher and the Servers page open narrowed to this
						organisation. The picker in the header changes it for one browser at a time.
					</p>
				</div>
			{/if}
		</div>
	</div>

	<div class="panel lg:col-span-2">
		<span class="label-sm">Delete account</span>
		<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
			<div class="space-y-2 text-[13px] leading-relaxed text-mist-400">
				<p>
					This removes your account, sign-in credentials, passkeys, sessions, server roles and
					organisation memberships straight away. It cannot be undone.
				</p>
				<p>
					Audit entries you caused are kept for the record but stripped of your name, IP address and
					browser. Organisations and servers you created stay with their other owners. You cannot
					delete your account while you are the only owner of an organisation, or the only site
					owner.
				</p>
			</div>
			<form
				method="post"
				action="?/deleteAccount"
				class="space-y-3"
				use:enhance={async ({ cancel }) => {
					const ok = await confirmDialog(
						'Delete your account and everything it can sign in to? This cannot be undone.',
						{ title: 'Delete account', okLabel: 'Delete my account', danger: true }
					);
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
						><span class="field-label">Your password</span><input
							class="input"
							type="password"
							name="password"
							autocomplete="current-password"
							required
						/></label
					>
				{:else}
					<label class="block"
						><span class="field-label">Type your username (@{data.user.username}) to confirm</span
						><input
							class="input"
							type="text"
							name="confirm"
							autocomplete="off"
							spellcheck="false"
							required
						/></label
					>
				{/if}
				<button class="btn btn-danger" type="submit" disabled={busy}>Delete my account</button>
			</form>
		</div>
	</div>
</div>
