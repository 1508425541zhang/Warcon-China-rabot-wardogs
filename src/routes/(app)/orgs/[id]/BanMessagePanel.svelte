<script lang="ts">
	// The org's ban message on the Ban list page: a closed strip showing the message in force, which
	// opens (for owners) into the field, its placeholders and a preview of what a player is shown.
	import { invalidateAll } from '$app/navigation';
	import { untrack } from 'svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import {
		BAN_MESSAGE_VARS,
		DEFAULT_BAN_MESSAGE,
		MAX_BAN_MESSAGE,
		renderBanMessage,
		unknownBanVars
	} from '$lib/ban-message';

	let { org, banMessage, owner }: { org: { id: string }; banMessage: string; owner: boolean } =
		$props();

	let open = $state(false);
	let busy = $state(false);
	let draft = $state(untrack(() => banMessage));
	let field = $state<HTMLInputElement>();
	$effect(() => {
		draft = banMessage;
	});

	let unknown = $derived(unknownBanVars(draft));

	const DAY = 86400_000;
	const sample = (reason: string, days: number | null) => {
		const addedAt = new Date();
		return renderBanMessage(draft, {
			entryId: days ? '7k2f9a' : '3qx9bd',
			reason,
			addedByName: 'Hollis',
			addedAt,
			expiresAt: days ? new Date(addedAt.getTime() + days * DAY) : null
		});
	};
	let samples = $derived([
		['7 day ban', sample('Team killing', 7)],
		['Permanent ban', sample('Cheating', null)]
	]);

	function insert(name: string) {
		const el = field;
		if (!el) return;
		const at = el.selectionStart ?? el.value.length;
		el.setRangeText(`{${name}}`, at, el.selectionEnd ?? at, 'end');
		el.dispatchEvent(new Event('input', { bubbles: true }));
		el.focus();
	}

	async function save(value: string) {
		busy = true;
		try {
			await api('PATCH', `/api/orgs/${encodeURIComponent(org.id)}`, { banMessage: value });
			toast('Ban message saved. Bans placed from now on carry it.', 'ok');
			await invalidateAll();
			open = false;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<div class="mb-4 panel px-4 py-3.5 sm:px-5">
	<button
		type="button"
		class="flex min-h-6 w-full cursor-pointer items-center gap-3 text-left"
		aria-expanded={open}
		onclick={() => (open = !open)}
	>
		<span class="caps whitespace-nowrap text-mist-400">Ban message</span>
		<span class="min-w-0 flex-1 truncate font-mono text-[12px] text-mist-600"
			>{open ? '' : banMessage}</span
		>
		<span class="inline-flex items-center gap-1.5 caps text-mist-400">
			{open ? 'Close' : owner ? 'Edit' : 'Show'}
			<svg
				width="12"
				height="12"
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2.5"
				aria-hidden="true"><path d={open ? 'M6 15l6-6 6 6' : 'M6 9l6 6 6-6'} /></svg
			>
		</span>
	</button>

	{#if open}
		<form
			class="mt-4"
			onsubmit={(e) => {
				e.preventDefault();
				void save(draft);
			}}
		>
			<div class="flex flex-col gap-4 md:flex-row md:gap-6">
				<div class="min-w-0 flex-1">
					<label class="block"
						><span class="field-label">Message</span><input
							class="input font-mono text-[12.5px]"
							type="text"
							maxlength={MAX_BAN_MESSAGE}
							disabled={!owner}
							bind:this={field}
							bind:value={draft}
						/></label
					>
					{#if owner}
						<div class="mt-2 flex flex-wrap items-center gap-1 text-[12px] text-mist-600">
							<span class="mr-1">Insert</span>
							{#each BAN_MESSAGE_VARS as n (n)}
								<button
									type="button"
									class="chip cursor-pointer text-mist-100 transition hover:bg-white/12"
									title="Insert {'{' + n + '}'} at the caret"
									onclick={() => insert(n)}>{'{' + n + '}'}</button
								>
							{/each}
						</div>
					{/if}
					{#if unknown.length}
						<p class="note text-danger">
							Unknown placeholder {unknown.map((k) => `{${k}}`).join(', ')}.
						</p>
					{/if}
					<p class="note">
						<span class="font-mono text-mist-100">{'{admin}'}</span> shows the banning admin's name
						to the player and to anyone who can view a server's ban list. Leave the message as
						<span class="font-mono text-mist-100">{DEFAULT_BAN_MESSAGE}</span> to send the reason
						alone.
						{#if !owner}Only an owner of the organisation can change it.{/if}
					</p>
				</div>
				<div class="min-w-0 flex-1">
					<span class="field-label">What the player is shown</span>
					<div class="space-y-2.5 rounded-ctl border border-black bg-ink-950 px-3.5 py-3">
						{#each samples as [label, text] (label)}
							<div>
								<div class="mb-1 caps text-mist-600">{label}</div>
								<div class="font-mono text-[12.5px] leading-relaxed break-words">
									{text || '(nothing)'}
								</div>
							</div>
						{/each}
					</div>
					<p class="note">
						Sample values, times in UTC. Used for bans placed from now on; a ban already on a server
						keeps the text it was placed with, also when it is edited later.
					</p>
				</div>
			</div>
			{#if owner}
				<div class="mt-4 flex flex-wrap justify-end gap-2">
					<button
						type="button"
						class="btn btn-ghost"
						disabled={busy || banMessage === DEFAULT_BAN_MESSAGE}
						onclick={() => save(DEFAULT_BAN_MESSAGE)}>Reset to reason only</button
					>
					<button
						type="submit"
						class="btn btn-primary"
						disabled={busy || !!unknown.length || draft.trim() === banMessage}>Save</button
					>
				</div>
			{/if}
		</form>
	{/if}
</div>
