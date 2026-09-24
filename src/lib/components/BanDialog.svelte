<script lang="ts">
	// Ban a player: across the whole organisation (the org ban list, pushed to every server) or on
	// one server only. Used from the org ban list page, the players page and the dossier.
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import { DEFAULT_BAN_MESSAGE, renderBanMessage } from '$lib/ban-message';
	import { describeSync, EXPIRY_OPTIONS, expiryIso, REASON_PRESETS } from '$lib/lists';
	import { isSteamId, steamProfiles, type SteamProfile } from '$lib/steam-profiles';
	import type { ListSyncServer, ListSyncSummary } from '$lib/types';
	import Modal from './Modal.svelte';
	import SteamName from './SteamName.svelte';

	let {
		orgId,
		orgName = '该组织',
		steamId = '',
		name = '',
		server = null,
		canOrg,
		banMessage = null,
		onclose,
		ondone
	}: {
		orgId: string;
		orgName?: string;
		/** fixed when banning a known player; otherwise the dialog asks for one */
		steamId?: string;
		name?: string;
		/** the server the dialog was opened from, if any: offers the "this server only" scope */
		server?: { id: string; name: string } | null;
		/** may the user write to the org list? */
		canOrg: boolean;
		/** the org's ban message, where the page has it: the dialog then shows the text it makes */
		banMessage?: string | null;
		onclose: () => void;
		ondone: (scope: 'org' | 'server') => unknown;
	} = $props();

	// Initial values only: the dialog is created fresh each time it opens.
	let id = $state(untrack(() => steamId));
	let reason = $state('');
	let expiry = $state('0');
	let custom = $state('');
	let scope = $state<'org' | 'server'>(untrack(() => (canOrg ? 'org' : 'server')));
	let busy = $state(false);

	// What the player will be shown, once the org wraps the reason in more than the reason. The
	// uid comes from the entry, which does not exist yet.
	let shown = $derived.by(() => {
		if (!banMessage || banMessage === DEFAULT_BAN_MESSAGE) return '';
		const until = expiryIso(expiry, custom);
		return renderBanMessage(banMessage.replace(/\{uid\}/gi, 'B-······'), {
			entryId: '',
			reason: reason.trim(),
			addedByName: page.data.user?.username ?? '',
			addedAt: new Date(),
			expiresAt: until ? new Date(until) : null
		});
	});

	let who = $derived(name ? `${name} (${steamId})` : steamId || '玩家');
	// A typed id is looked up so the admin sees who they are about to ban.
	let previewId = $derived(steamId ? '' : isSteamId(id.trim()) ? id.trim() : '');
	let preview = $state<SteamProfile | null | undefined>(undefined);
	$effect(() => {
		const want = previewId;
		preview = undefined;
		if (!want) return;
		void steamProfiles([want]).then((r) => {
			if (previewId === want && want in r) preview = r[want];
		});
	});

	async function submit() {
		const target = id.trim();
		if (!/^\d{17}$/.test(target)) {
			toast('请输入 17 位 SteamID64。', 'err');
			return;
		}
		busy = true;
		try {
			if (scope === 'org') {
				const res = await api<{ sync: ListSyncSummary }>(
					'POST',
					`/api/orgs/${encodeURIComponent(orgId)}/lists/ban/entries`,
					{ steamId: target, reason: reason.trim(), expiresAt: expiryIso(expiry, custom) }
				);
				toast(describeSync(res.sync, `已在“${orgName}”组织中封禁 ${target}。`), 'ok', 8000);
			} else if (server) {
				const res = await api<{ sync: ListSyncServer }>(
					'POST',
					`/api/servers/${encodeURIComponent(server.id)}/lists/ban/entries`,
					{ steamId: target, reason: reason.trim(), expiresAt: expiryIso(expiry, custom) }
				);
				// The game only bans a connected player; the list keeps the ban for when they join.
				toast(
					res.sync.ok && res.sync.failed
						? `${target} 当前不在 ${server.name}，下次加入时将被封禁。`
						: describeSync({ servers: [res.sync] }, `已在 ${server.name} 封禁 ${target}。`),
					'ok',
					8000
				);
			}
			await ondone(scope);
			onclose();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<Modal title="Ban {who}" {onclose}>
	<form
		class="space-y-3"
		onsubmit={(e) => {
			e.preventDefault();
			void submit();
		}}
	>
		{#if !steamId}
			<label class="block"
				><span class="field-label">SteamID64</span><input
					class="input font-mono"
					type="text"
					inputmode="numeric"
					placeholder="7656119…"
					maxlength="17"
					bind:value={id}
					required
				/></label
			>
			{#if previewId && preview}
				<div class="mt-1.5 text-[12.5px]"><SteamName profile={preview} /></div>
			{:else if previewId && preview === null}
				<div class="mt-1.5 text-[12.5px] text-mist-600">找不到该 SteamID 的资料。</div>
			{/if}
		{/if}

		{#if server && canOrg}
			<fieldset class="space-y-1.5">
				<legend class="field-label">位置</legend>
				<label class="flex items-start gap-2">
					<input type="radio" class="mt-1" bind:group={scope} value="org" />
					<span
						><b>以下组织的每台服务器： {orgName}</b>
						<span class="block text-[12.5px] text-mist-400"
							>加入组织封禁列表，立即和将来都同步到组织所有服务器。</span
						></span
					>
				</label>
				<label class="flex items-start gap-2">
					<input type="radio" class="mt-1" bind:group={scope} value="server" />
					<span
						><b>{server.name} 仅限</b>
						<span class="block text-[12.5px] text-mist-400"
							>加入此服务器的封禁列表。如果玩家当前不在线，将在下次进入时封禁。</span
						></span
					>
				</label>
			</fieldset>
		{:else if server}
			<p class="note">
				加入 {server.name}的封禁列表。如果玩家当前不在线，将在下次进入时封禁。
			</p>
		{/if}

		<label class="block"
			><span class="field-label">原因</span><input
				class="input"
				type="text"
				placeholder="可选，显示在服务器封禁名单中"
				maxlength="200"
				bind:value={reason}
			/></label
		>
		<div class="flex flex-wrap gap-1.5">
			{#each REASON_PRESETS as preset (preset)}
				<button
					type="button"
					class="chip cursor-pointer hover:bg-white/12 {reason === preset ? 'text-accent' : ''}"
					onclick={() => (reason = preset)}>{preset}</button
				>
			{/each}
		</div>

		<div class="flex flex-wrap gap-3">
			<label class="block sm:w-48"
				><span class="field-label">到期时间</span><select class="input" bind:value={expiry}>
					{#each EXPIRY_OPTIONS as [value, label] (value)}
						<option {value}>{label}</option>
					{/each}
				</select></label
			>
			{#if expiry === 'custom'}
				<label class="block sm:flex-1"
					><span class="field-label">截止时间（本地）</span><input
						class="input"
						type="datetime-local"
						bind:value={custom}
						required
					/></label
				>
			{/if}
		</div>

		{#if shown}
			<div>
				<span class="field-label">玩家可见内容</span>
				<div
					class="rounded-ctl border border-black bg-ink-950 px-3.5 py-2.5 font-mono text-[12.5px] leading-relaxed break-words"
				>
					{shown}
				</div>
				<p class="note">从 {orgName}的封禁消息。</p>
			</div>
		{/if}

		<div class="flex justify-end gap-2 pt-2">
			<button type="button" class="btn" data-close onclick={onclose}>取消</button>
			<button type="submit" class="btn btn-danger" disabled={busy}>封禁</button>
		</div>
	</form>
</Modal>
