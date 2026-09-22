<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	type Setting = (typeof data.settings)[number];
	/** edits by key, in the unit shown (seconds for millisecond settings) */
	let edits = $state<Record<string, string>>({});
	let busy = $state(false);

	const GROUPS: { id: Setting['group']; title: string; blurb: string }[] = [
		{
			id: 'observation',
			title: 'Observation cadence',
			blurb:
				'How often the worker looks at each server. Watched: someone has it open. Busy: people on it. The game listener sets the floor; lower is fresher.'
		},
		{
			id: 'delivery',
			title: 'Trigger delivery',
			blurb: 'How trigger actions are sent and when a late one is dropped instead.'
		},
		{
			id: 'housekeeping',
			title: 'Housekeeping',
			blurb: 'Database writes that are not observations. History is never deleted.'
		},
		{
			id: 'accounts',
			title: 'Accounts and sign-in',
			blurb:
				'Who the sign-in rules (two ways in, a second factor on any password; see the account page) are enforced on, and how long an account may fall short before the panel is limited to its account page.'
		}
	];

	const shown = (s: Setting, v = s.value) => (s.unit === 'ms' ? String(v / 1000) : String(v));
	const unitLabel = (s: Setting) => (s.unit === 'ms' ? 's' : s.unit === 'days' ? 'days' : '');
	const bounds = (s: Setting) =>
		s.unit === 'choice'
			? ''
			: s.unit === 'ms'
				? `${s.min / 1000}–${s.max / 1000} s`
				: `${s.min}–${s.max}`;
	const optionLabel = (s: Setting, v: number) =>
		s.options?.find((o) => o.value === v)?.label ?? String(v);
	const value = (s: Setting) => (s.key in edits ? edits[s.key] : shown(s));
	const dirty = (s: Setting) => s.key in edits && edits[s.key] !== shown(s);

	async function save() {
		const values: Record<string, number> = {};
		for (const s of data.settings) {
			if (!dirty(s)) continue;
			const n = Number(edits[s.key]);
			if (!Number.isFinite(n)) {
				toast(`${s.label}: not a number.`, 'err');
				return;
			}
			values[s.key] = s.unit === 'ms' ? Math.round(n * 1000) : Math.round(n);
		}
		if (!Object.keys(values).length) return;
		busy = true;
		try {
			await api('PUT', '/api/settings', { values });
			toast('Settings saved; the worker picks them up within ten seconds.', 'ok');
			edits = {};
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}

	async function reset(s: Setting) {
		busy = true;
		try {
			await api('PUT', '/api/settings', { reset: [s.key] });
			delete edits[s.key];
			await invalidateAll();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Settings · Admin · {data.appName}</title></svelte:head>

{#each GROUPS as g (g.id)}
	<div class="mb-4 panel">
		<span class="label-sm">{g.title}</span>
		<p class="mb-3 text-[13px] text-mist-400">{g.blurb}</p>
		<div class="table-wrap">
			<table>
				<thead><tr><th>Setting</th><th>Value</th><th>Allowed</th><th></th></tr></thead>
				<tbody>
					{#each data.settings.filter((s) => s.group === g.id) as s (s.key)}
						<tr>
							<td>
								<div class="font-medium">{s.label}</div>
								<div class="text-[12.5px] text-mist-500">{s.help}</div>
							</td>
							<td class="whitespace-nowrap">
								{#if s.options}
									<select
										class="input pr-[30px]"
										value={value(s)}
										onchange={(e) => (edits[s.key] = (e.target as HTMLSelectElement).value)}
									>
										{#each s.options as o (o.value)}<option value={String(o.value)}
												>{o.label}</option
											>{/each}
									</select>
								{:else}
									<span class="join">
										<input
											class="input w-28"
											type="number"
											step={s.unit === 'ms' ? 0.5 : 1}
											value={value(s)}
											oninput={(e) => (edits[s.key] = (e.target as HTMLInputElement).value)}
										/>
										<span class="pointer-events-none btn btn-ghost">{unitLabel(s)}</span>
									</span>
								{/if}
								{#if dirty(s)}<Badge tone="warn" class="ml-1">unsaved</Badge>{/if}
							</td>
							<td class="whitespace-nowrap text-mist-500">{bounds(s)}</td>
							<td class="whitespace-nowrap">
								{#if s.stored}
									<button class="btn btn-sm" disabled={busy} onclick={() => reset(s)}
										>Reset to {s.options
											? optionLabel(s, s.default)
											: shown(s, s.default) + unitLabel(s)}</button
									>
								{:else}<span class="text-[12.5px] text-mist-600">default</span>{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
{/each}

<div class="flex items-center gap-3">
	<button class="btn btn-primary" disabled={busy || !data.settings.some(dirty)} onclick={save}
		>Save changes</button
	>
	<span class="text-[13px] text-mist-500"
		>Changes are audited and take effect without a restart.</span
	>
</div>
