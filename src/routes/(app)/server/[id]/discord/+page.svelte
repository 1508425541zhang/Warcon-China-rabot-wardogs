<script lang="ts">
	// Where this server's live status card goes: the channels that carry it, and the form that adds
	// one. Adding creates a webhook restricted to this server that keeps status cards and mirrors
	// nothing; the org page lists it with the rest.
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
	let orgPath = $derived(`/orgs/${encodeURIComponent(data.server.orgId)}`);
	let label = $state('');
	let url = $state('');
	let style = $state<StatusStyle>('banner');
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
		await run(
			() =>
				api('POST', `${orgPath}/webhooks`, {
					label: label.trim() || `${data.server.name} status`,
					url: url.trim(),
					events: [],
					statusEnabled: true,
					statusStyle: style,
					serverIds: [data.server.id]
				}),
			'Channel connected. The card is on its way; pin it in Discord once it lands.'
		);
		label = '';
		url = '';
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
	/** A webhook this page made: only this server, no mirrored events. Others belong to the org page. */
	const ownHere = (w: WebhookView) => !w.events.length && w.serverIds?.length === 1;
</script>

<div class="panel">
	<div class="mb-3 flex items-center gap-3">
		<span class="label-sm mb-0!">Discord status card</span>
	</div>
	<p class="mb-3 text-[13px] text-mist-400">
		A card in a channel that Warcon keeps up to date: players online, map, a score bar per faction
		and who is on each side, with the map art. It is posted once and edited in place. <b
			>Pin it in Discord</b
		> (message menu → Pin Message) so it stays at the top of the channel.
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
						{#if !w.serverIds}Every server in the organisation{:else if w.serverIds.length > 1}This
							and {w.serverIds.length - 1} other server{w.serverIds.length === 2
								? ''
								: 's'}{:else}This server only{/if}
						{#if w.events.length}· also mirrors events{/if}
						{#if !ownHere(w)}· {w.statusStyle} card · set up on the org page{/if}
						{#if w.lastError}<div class="text-danger">{w.lastError}</div>
							<div>{hintFor(w.lastError)}</div>{:else if w.statusSentAt}· updated {fmtTime(
								w.statusSentAt
							)}{/if}
					</div>
				</div>
				<span class="inline-flex shrink-0 flex-wrap justify-end gap-1.5">
					<button class="btn btn-sm" onclick={() => testCard(w)} disabled={busy || !w.enabled}
						>Test card</button
					>
					{#if ownHere(w)}
						<select
							class="input w-36"
							value={w.statusStyle}
							disabled={busy}
							aria-label="Card style"
							onchange={(e) => setStyle(w, e.currentTarget.value)}
						>
							{#each STATUS_STYLES as st (st)}<option value={st}>{st}</option>{/each}
						</select>
						<button class="btn btn-sm" onclick={() => toggle(w)} disabled={busy}
							>{w.enabled ? 'Pause' : 'Enable'}</button
						>
						<button class="btn btn-sm btn-danger" onclick={() => remove(w)} disabled={busy}
							>Disconnect</button
						>
					{:else}
						<a class="btn btn-sm" href={orgPath}>Edit on the org page</a>
					{/if}
				</span>
			</div>
		{:else}
			<p class="mb-3 text-[13px] text-mist-600">No channel carries this server's card yet.</p>
		{/each}

		<form
			class="mt-4 space-y-3 border-t border-white/8 pt-4"
			onsubmit={(e) => {
				e.preventDefault();
				void add();
			}}
		>
			<span class="field-label">Connect a channel</span>
			<div class="grid gap-3 sm:grid-cols-[1fr_2fr_1fr]">
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
				<label class="block"
					><span class="field-label">Card style</span><select
						id="discord-style"
						class="input"
						bind:value={style}
					>
						{#each STATUS_STYLES as st (st)}<option value={st}>{st}</option>{/each}
					</select></label
				>
			</div>
			<p class="note">{STATUS_STYLE_LABELS[style]}</p>
			<p class="note">
				In Discord, open the channel's settings → Integrations → Webhooks → New Webhook, copy its
				URL and paste it here. The URL is stored encrypted and never shown again. Pictures need the
				panel to be reachable over https.
			</p>
			<div class="flex justify-end">
				<button type="submit" class="btn btn-primary" disabled={busy}>Connect channel</button>
			</div>
		</form>
	{/if}
</div>
