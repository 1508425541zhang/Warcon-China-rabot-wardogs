<script lang="ts">
	// This server's Discord channels: the ones that carry its live status card or its team kills,
	// and the form that connects another. A channel connected here is a webhook restricted to this
	// server carrying only those two things; the org page lists it with the rest and is where the
	// audit mirror (bans, kicks, sign-ins) is set up.
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import type { WebhookView } from '$lib/types';
	import { STATUS_STYLE_LABELS, STATUS_STYLES, type StatusStyle } from '$lib/status-styles';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let orgPage = $derived(`/orgs/${encodeURIComponent(data.server.orgId)}`);
	let orgPath = $derived(`/api${orgPage}`);
	let label = $state('');
	let url = $state('');
	let style = $state<StatusStyle>('banner');
	let wantCard = $state(true);
	let wantTeamKills = $state(false);
	let busy = $state(false);

	async function run(fn: () => Promise<unknown>, done: string) {
		busy = true;
		try {
			await fn();
			toast(done, 'ok');
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
	async function add() {
		if (!wantCard && !wantTeamKills) return;
		await run(
			() =>
				api('POST', `${orgPath}/webhooks`, {
					label: label.trim() || `${data.server.name} ${wantCard ? 'status' : 'team kills'}`,
					url: url.trim(),
					events: wantTeamKills ? ['teamkills'] : [],
					statusEnabled: wantCard,
					statusStyle: style,
					serverIds: [data.server.id]
				}),
			wantCard
				? 'Channel connected. The card is on its way; pin it in Discord once it lands.'
				: 'Channel connected. Team kills will be posted as the kill feed reports them.'
		);
		label = '';
		url = '';
	}
	/** what a channel carries, in words */
	const carries = (w: WebhookView): string[] => {
		const out: string[] = [];
		if (w.statusEnabled) out.push(`status card (${w.statusStyle})`);
		if (w.events.includes('teamkills')) out.push('team kills');
		const mirrored = w.events.filter((e) => e !== 'teamkills').length;
		if (mirrored) out.push(`${mirrored} kind${mirrored === 1 ? '' : 's'} of admin events`);
		return out;
	};
	function toggleTeamKills(w: WebhookView) {
		const on = !w.events.includes('teamkills');
		void run(
			() =>
				api('PATCH', `${orgPath}/webhooks/${w.id}`, {
					events: on ? [...w.events, 'teamkills'] : w.events.filter((e) => e !== 'teamkills')
				}),
			on ? 'Team kills will be posted to this channel.' : 'Team kills stop going to this channel.'
		);
	}
	function toggleCard(w: WebhookView) {
		void run(
			() => api('PATCH', `${orgPath}/webhooks/${w.id}`, { statusEnabled: !w.statusEnabled }),
			w.statusEnabled ? 'Card removed from the channel.' : 'Card on its way; pin it once it lands.'
		);
	}
	function testCard(w: WebhookView) {
		void run(
			() => api('POST', `${orgPath}/webhooks/${w.id}/card`, { serverId: data.server.id }),
			'Test card sent. It disappears in a minute.'
		);
	}
	/** What to do about a failure, from Discord's answer. */
	const hintFor = (error: string): string =>
		/404|Unknown Webhook|401|403/i.test(error)
			? 'Discord no longer knows this webhook. Disconnect it and connect a new one.'
			: /rate limit/i.test(error)
				? 'Discord is rate limiting the channel; Warcon backs off and retries.'
				: 'Warcon retries every minute.';
	function setStyle(w: WebhookView, statusStyle: string) {
		void run(
			() => api('PATCH', `${orgPath}/webhooks/${w.id}`, { statusStyle }),
			'Style changed. The card updates within a minute.'
		);
	}
	function toggle(w: WebhookView) {
		void run(
			() => api('PATCH', `${orgPath}/webhooks/${w.id}`, { enabled: !w.enabled }),
			w.enabled ? 'Card paused and removed from the channel.' : 'Card enabled.'
		);
	}
	async function remove(w: WebhookView) {
		if (!(await confirmDialog(`Disconnect ${w.label}?`, { okLabel: 'Disconnect', danger: true })))
			return;
		await run(() => api('DELETE', `${orgPath}/webhooks/${w.id}`), 'Channel disconnected.');
	}
	/** A channel this page can manage: this server only, carrying nothing but the card and team kills. */
	const ownHere = (w: WebhookView) =>
		w.serverIds?.length === 1 && w.events.every((e) => e === 'teamkills');
</script>

<div class="panel">
	<div class="mb-3 flex items-center gap-3">
		<span class="label-sm mb-0!">Discord channels</span>
	</div>
	<p class="mb-3 text-[13px] text-mist-400">
		A channel is one Discord webhook URL, and each channel carries what you choose for it. Two
		things belong here: a <b>status card</b> that Warcon posts once and edits in place (players
		online, map, a score bar per faction and who is on each side; <b>pin it in Discord</b> so it
		stays at the top), and <b>team kills</b>, one message each as the kill feed reports them. Use a
		separate channel for each if you want them apart. Mirrors of admin actions (bans, kicks, trigger
		actions, sign-ins) are set up on the <a class="link" href={orgPage}>org page</a>.
	</p>
	{#if data.owner && !data.https}
		<div class="callout mb-3 border-warn/30 bg-warn/12">
			<b>Cards will go out without pictures.</b> Discord only fetches map art and icons over https,
			and this panel is on {new URL(location.href).protocol.replace(':', '')}. Everything else on
			the card works.
		</div>
	{/if}
	{#if !data.owner}
		<p class="note">
			Only an owner of {data.server.orgName} can connect Discord channels, because a webhook URL lets
			anyone post there.
		</p>
	{:else}
		{#each data.channels as w (w.id)}
			<div class="kv items-start">
				<div class="min-w-0">
					<div>
						{w.label}
						{#if !w.enabled}<Badge class="ml-1">paused</Badge>{/if}
						{#if w.lastError}<Badge tone="err" class="ml-1">failing</Badge
							>{:else if w.statusSentAt}<Badge tone="ok" class="ml-1">live</Badge>{/if}
					</div>
					<div class="truncate font-mono text-[11px] text-mist-600">{w.urlHint}</div>
					<div class="text-[12px] text-mist-400">
						Carries {carries(w).join(', ') || 'nothing'} ·
						{#if !w.serverIds}every server in the organisation{:else if w.serverIds.length > 1}this
							and {w.serverIds.length - 1} other server{w.serverIds.length === 2
								? ''
								: 's'}{:else}this server only{/if}
						{#if !ownHere(w)}· set up on the org page{/if}
						{#if w.lastError}<div class="text-danger">{w.lastError}</div>
							<div>{hintFor(w.lastError)}</div>{:else if w.statusSentAt}· updated {fmtTime(
								w.statusSentAt
							)}{/if}
					</div>
				</div>
				<span class="inline-flex shrink-0 flex-wrap justify-end gap-1.5">
					{#if w.statusEnabled}
						<button class="btn btn-sm" onclick={() => testCard(w)} disabled={busy || !w.enabled}
							>Test card</button
						>
					{/if}
					{#if ownHere(w)}
						<button class="btn btn-sm" onclick={() => toggleCard(w)} disabled={busy}
							>{w.statusEnabled ? 'Card: on' : 'Card: off'}</button
						>
						<button class="btn btn-sm" onclick={() => toggleTeamKills(w)} disabled={busy}
							>{w.events.includes('teamkills') ? 'Team kills: on' : 'Team kills: off'}</button
						>
						{#if w.statusEnabled}
							<select
								class="input w-36"
								value={w.statusStyle}
								disabled={busy}
								aria-label="Card style"
								onchange={(e) => setStyle(w, e.currentTarget.value)}
							>
								{#each STATUS_STYLES as st (st)}<option value={st}>{st}</option>{/each}
							</select>
						{/if}
						<button class="btn btn-sm" onclick={() => toggle(w)} disabled={busy}
							>{w.enabled ? 'Pause' : 'Enable'}</button
						>
						<button class="btn btn-sm btn-danger" onclick={() => remove(w)} disabled={busy}
							>Disconnect</button
						>
					{:else}
						<a class="btn btn-sm" href={orgPage}>Edit on the org page</a>
					{/if}
				</span>
			</div>
		{:else}
			<p class="mb-3 text-[13px] text-mist-600">
				No channel carries this server's card or team kills yet.
			</p>
		{/each}

		<form
			class="mt-4 space-y-3 border-t border-white/8 pt-4"
			onsubmit={(e) => {
				e.preventDefault();
				void add();
			}}
		>
			<span class="field-label">Connect a channel</span>
			<div class="grid gap-3 sm:grid-cols-[1fr_2fr]">
				<label class="block"
					><span class="field-label">Label</span><input
						id="discord-label"
						class="input"
						type="text"
						bind:value={label}
						placeholder="e.g. #eu-1-status"
						maxlength="60"
					/></label
				>
				<label class="block"
					><span class="field-label">Webhook URL</span><input
						id="discord-url"
						class="input font-mono text-[12.5px]"
						type="url"
						bind:value={url}
						placeholder="https://discord.com/api/webhooks/…"
						required
						autocomplete="off"
					/></label
				>
			</div>
			<div class="flex flex-wrap items-center gap-x-6 gap-y-2">
				<span class="field-label mb-0">This channel carries</span>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={wantCard} /> Live status card</label
				>
				<label class="flex items-center gap-2 text-[13px]"
					><input type="checkbox" bind:checked={wantTeamKills} /> Team kills</label
				>
				{#if wantCard}
					<select id="discord-style" class="input w-40" bind:value={style} aria-label="Card style">
						{#each STATUS_STYLES as st (st)}<option value={st}>{st}</option>{/each}
					</select>
				{/if}
			</div>
			{#if wantCard}<p class="note">{STATUS_STYLE_LABELS[style]}</p>{/if}
			{#if wantTeamKills}<p class="note">
					Team kills need the kill feed, set up on the Configuration tab.
				</p>{/if}
			<p class="note">
				In Discord, open the channel's settings → Integrations → Webhooks → New Webhook, copy its
				URL and paste it here. The URL is stored encrypted and never shown again. Pictures need the
				panel to be reachable over https.
			</p>
			<div class="flex justify-end">
				<button
					type="submit"
					class="btn btn-primary"
					disabled={busy || (!wantCard && !wantTeamKills)}>Connect channel</button
				>
			</div>
		</form>
	{/if}
</div>
