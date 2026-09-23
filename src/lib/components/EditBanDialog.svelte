<script lang="ts">
	// Change the reason or the expiry of a ban the panel holds, on the organisation's list or on a
	// server's own. Who placed it and when stay as they are.
	import { untrack } from 'svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import { toDatetimeLocal } from '$lib/format';
	import { EXPIRY_OPTIONS, expiryIso } from '$lib/lists';
	import Modal from './Modal.svelte';

	let {
		path,
		who,
		placed,
		reason: reasonNow,
		expiresAt,
		onclose,
		ondone
	}: {
		/** the entry's API path: the org's list or the server's own */
		path: string;
		who: string;
		/** one line on where the ban is, who placed it and when */
		placed: string;
		reason: string;
		expiresAt: string | null;
		onclose: () => void;
		ondone: () => unknown;
	} = $props();

	// Initial values only: the dialog is created fresh each time it opens.
	let reason = $state(untrack(() => reasonNow));
	let expiry = $state(untrack(() => (expiresAt ? 'custom' : '0')));
	let custom = $state(untrack(() => toDatetimeLocal(expiresAt)));
	// The expiry goes out only when it was touched: the form holds it to the minute, and a ban
	// already past it would be refused as not in the future.
	const expiryWas = untrack(() => [expiry, custom].join());
	let busy = $state(false);

	async function submit() {
		busy = true;
		try {
			await api('PATCH', path, {
				reason: reason.trim(),
				...([expiry, custom].join() === expiryWas ? {} : { expiresAt: expiryIso(expiry, custom) })
			});
			toast(`Ban on ${who} changed.`, 'ok');
			await ondone();
			onclose();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<Modal title="Edit ban: {who}" {onclose}>
	<form
		class="space-y-3"
		onsubmit={(e) => {
			e.preventDefault();
			void submit();
		}}
	>
		<p class="note mt-0!">{placed} Who placed it and when stay as they are.</p>
		<label class="block"
			><span class="field-label">Reason</span><input
				class="input"
				type="text"
				maxlength="200"
				bind:value={reason}
			/></label
		>
		<div class="flex flex-wrap gap-3">
			<label class="block sm:w-48"
				><span class="field-label">Expires</span><select class="input" bind:value={expiry}>
					{#each EXPIRY_OPTIONS as [value, label] (value)}
						<option {value}>{label}</option>
					{/each}
				</select></label
			>
			{#if expiry === 'custom'}
				<label class="block sm:flex-1"
					><span class="field-label">Until (local time)</span><input
						class="input"
						type="datetime-local"
						bind:value={custom}
						required
					/></label
				>
			{/if}
		</div>
		<div class="flex justify-end gap-2 pt-2">
			<button type="button" class="btn" data-close onclick={onclose}>Cancel</button>
			<button type="submit" class="btn btn-primary" disabled={busy}>Save</button>
		</div>
	</form>
</Modal>
