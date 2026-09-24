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
		['封禁 7 天', sample('队友击杀', 7)],
		['永久封禁', sample('Cheating', null)]
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
			toast('封禁消息已保存，之后的封禁将使用此消息。', 'ok');
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
		<span class="caps whitespace-nowrap text-mist-400">封禁消息</span>
		<span class="min-w-0 flex-1 truncate font-mono text-[12px] text-mist-600"
			>{open ? '' : banMessage}</span
		>
		<span class="inline-flex items-center gap-1.5 caps text-mist-400">
			{open ? '收起' : owner ? '编辑' : '查看'}
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
						><span class="field-label">消息</span><input
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
							<span class="mr-1">插入</span>
							{#each BAN_MESSAGE_VARS as n (n)}
								<button
									type="button"
									class="chip cursor-pointer text-mist-100 transition hover:bg-white/12"
									title="在光标处插入 {'{' + n + '}'}"
									onclick={() => insert(n)}>{'{' + n + '}'}</button
								>
							{/each}
						</div>
					{/if}
					{#if unknown.length}
						<p class="note text-danger">
							未知占位符 {unknown.map((k) => `{${k}}`).join(', ')}.
						</p>
					{/if}
					<p class="note">
						<span class="font-mono text-mist-100">{'{admin}'}</span>
						会向被封禁玩家和有权查看服务器封禁列表的人显示执行管理员的姓名。将消息设为
						<span class="font-mono text-mist-100">{DEFAULT_BAN_MESSAGE}</span> 则只发送原因。
						{#if !owner}只有组织所有者可以修改。{/if}
					</p>
				</div>
				<div class="min-w-0 flex-1">
					<span class="field-label">玩家可见内容</span>
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
						以下是示例值，时间使用
						UTC。设置仅适用于之后执行的封禁；服务器上已有的封禁即使之后被编辑，也会保留原有文本。
					</p>
				</div>
			</div>
			{#if owner}
				<div class="mt-4 flex flex-wrap justify-end gap-2">
					<button
						type="button"
						class="btn btn-ghost"
						disabled={busy || banMessage === DEFAULT_BAN_MESSAGE}
						onclick={() => save(DEFAULT_BAN_MESSAGE)}>恢复为仅显示原因</button
					>
					<button
						type="submit"
						class="btn btn-primary"
						disabled={busy || !!unknown.length || draft.trim() === banMessage}>保存</button
					>
				</div>
			{/if}
		</form>
	{/if}
</div>
