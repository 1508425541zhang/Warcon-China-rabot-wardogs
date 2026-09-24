<script lang="ts">
	// This server's Discord channels (a section of the Settings tab): the ones that carry its live
	// status card or its team kills, and the form that connects another. A channel connected here
	// is a webhook restricted to this server carrying only those two things; the org page lists it
	// with the rest and is where the audit mirror (bans, kicks, sign-ins) is set up. Each row is a
	// line of text with one Edit button; changing, testing, pausing and disconnecting a channel
	// happen in its dialog.
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import { fmtTime } from '$lib/format';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import type { ServerInfo, WebhookView } from '$lib/types';
	import type { StatusStyle } from '$lib/status-styles';
	import { effectiveFeatures } from '$lib/features';
	import ChannelFields from './ChannelFields.svelte';

	type Carry = 'card' | 'teamkills' | 'both';
	type CardSettings = {
		style: StatusStyle;
		interval: number;
		linkStatus: boolean;
		linkLeaderboard: boolean;
		linkPanel: boolean;
	};

	let {
		data
	}: { data: { server: ServerInfo; owner: boolean; https: boolean; channels: WebhookView[] } } =
		$props();
	let orgPage = $derived(`/orgs/${encodeURIComponent(data.server.orgId)}`);
	let orgPath = $derived(`/api${orgPage}`);
	let features = $derived(effectiveFeatures(data.server, data.server));
	let label = $state('');
	let url = $state('');
	/** what the new channel is for; the card settings only matter when a card is part of it */
	let carry = $state<Carry>('card');
	let card = $state<CardSettings>({
		style: 'banner',
		interval: 60,
		linkStatus: true,
		linkLeaderboard: true,
		linkPanel: false
	});
	let wantCard = $derived(carry !== 'teamkills');
	let wantTeamKills = $derived(carry !== 'card');
	let busy = $state(false);
	/** the channel whose dialog is open, with the settings as edited so far */
	let editing = $state<{ w: WebhookView; label: string; carry: Carry; card: CardSettings } | null>(
		null
	);

	async function run(fn: () => Promise<unknown>, done: string): Promise<boolean> {
		busy = true;
		try {
			await fn();
			toast(done, 'ok');
			await invalidateAll();
			return true;
		} catch (err) {
			toast(errorMessage(err), 'err');
			return false;
		} finally {
			busy = false;
		}
	}
	const cardBody = (c: CardSettings) => ({
		statusStyle: c.style,
		statusIntervalS: c.interval,
		linkStatus: c.linkStatus,
		linkLeaderboard: c.linkLeaderboard,
		linkPanel: c.linkPanel
	});
	async function add() {
		await run(
			() =>
				api('POST', `${orgPath}/webhooks`, {
					label: label.trim() || `${data.server.name} ${wantCard ? '状态' : '队友击杀'}`,
					url: url.trim(),
					events: wantTeamKills ? ['teamkills'] : [],
					statusEnabled: wantCard,
					...cardBody(card),
					serverIds: [data.server.id]
				}),
			wantCard
				? '频道已连接，卡片即将发送。出现后请在 Discord 中置顶。'
				: '频道已连接，收到队友击杀事件时会发送通知。'
		);
		label = '';
		url = '';
	}
	/** What a channel carries, in words: "Status card (banner) and team kills". */
	const carries = (w: WebhookView): string => {
		const parts: string[] = [];
		if (w.statusEnabled) parts.push(`状态卡片（${w.statusStyle}，每 ${w.statusIntervalS} 秒）`);
		if (w.events.includes('teamkills')) parts.push('队友击杀');
		const mirrored = w.events.filter((e) => e !== 'teamkills').length;
		if (mirrored) parts.push(`${mirrored} 类管理员事件`);
		const text =
			parts.length > 1
				? `${parts.slice(0, -1).join('、')}和${parts[parts.length - 1]}`
				: (parts[0] ?? '无');
		return text;
	};
	/** The links a card carries right now, given the pages that are on. */
	const linksOf = (w: WebhookView): string => {
		const on = [
			w.linkStatus && features.status ? '实时状态' : '',
			w.linkLeaderboard && features.leaderboards ? '排行榜' : '',
			w.linkPanel ? '管理面板' : ''
		].filter(Boolean);
		return on.length ? `链接至${on.join('、')}` : '无链接';
	};
	const carryOf = (w: WebhookView): Carry =>
		w.statusEnabled ? (w.events.includes('teamkills') ? 'both' : 'card') : 'teamkills';
	function openEdit(w: WebhookView) {
		editing = {
			w,
			label: w.label,
			carry: carryOf(w),
			card: {
				style: w.statusStyle,
				interval: w.statusIntervalS,
				linkStatus: w.linkStatus,
				linkLeaderboard: w.linkLeaderboard,
				linkPanel: w.linkPanel
			}
		};
	}
	/** Sends only what changed; a channel edited here carries nothing but the card and team kills. */
	async function save() {
		const e = editing;
		if (!e) return;
		const wantsCard = e.carry !== 'teamkills';
		const teamKills = e.carry !== 'card';
		const body: Record<string, unknown> = {};
		const name = e.label.trim();
		if (name && name !== e.w.label) body.label = name;
		if (wantsCard !== e.w.statusEnabled) body.statusEnabled = wantsCard;
		if (wantsCard) {
			const now = cardBody(e.card);
			const was = cardBody({
				style: e.w.statusStyle,
				interval: e.w.statusIntervalS,
				linkStatus: e.w.linkStatus,
				linkLeaderboard: e.w.linkLeaderboard,
				linkPanel: e.w.linkPanel
			});
			for (const k of Object.keys(now) as (keyof typeof now)[])
				if (now[k] !== was[k]) body[k] = now[k];
		}
		if (teamKills !== e.w.events.includes('teamkills'))
			body.events = teamKills ? ['teamkills'] : [];
		if (!Object.keys(body).length) {
			editing = null;
			return;
		}
		const ok = await run(
			() => api('PATCH', `${orgPath}/webhooks/${e.w.id}`, body),
			body.statusEnabled === true
				? '频道已更新，卡片即将发送。出现后请在 Discord 中置顶。'
				: '频道已更新。'
		);
		if (ok) editing = null;
	}
	function testCard(w: WebhookView) {
		void run(
			() => api('POST', `${orgPath}/webhooks/${w.id}/card`, { serverId: data.server.id }),
			'测试卡片已发送，将在一分钟后消失。'
		);
	}
	/** What to do about a failure, from Discord's answer. */
	const hintFor = (error: string): string =>
		/404|Unknown Webhook|401|403/i.test(error)
			? 'Discord 已无法识别此网络钩子，请断开后重新连接。'
			: /rate limit/i.test(error)
				? 'Discord 正在限制此频道的发送速率，Warcon 将延后重试。'
				: 'Warcon 每分钟重试一次。';
	async function toggle(w: WebhookView) {
		const ok = await run(
			() => api('PATCH', `${orgPath}/webhooks/${w.id}`, { enabled: !w.enabled }),
			w.enabled
				? w.statusEnabled
					? '频道已暂停，卡片会撤下；重新启用前不会发送消息。'
					: '频道已暂停，重新启用前不会发送消息。'
				: w.statusEnabled
					? '频道已启用，卡片即将发送。出现后请置顶。'
					: '频道已启用。'
		);
		if (ok) editing = null;
	}
	async function remove(w: WebhookView) {
		// One dialog at a time: the confirmation replaces the edit dialog rather than stacking on it.
		editing = null;
		if (!(await confirmDialog(`Disconnect ${w.label}?`, { okLabel: 'Disconnect', danger: true })))
			return;
		await run(() => api('DELETE', `${orgPath}/webhooks/${w.id}`), '频道已断开。');
	}
	/** A channel this page can manage: this server only, carrying nothing but the card and team kills. */
	const ownHere = (w: WebhookView) =>
		w.serverIds?.length === 1 && w.events.every((e) => e === 'teamkills');
</script>

<div class="panel">
	<div class="mb-3 flex items-center gap-3">
		<span class="label-sm mb-0!">Discord 频道</span>
	</div>
	<p class="mb-3 text-[13px] text-mist-400">
		每个频道对应一个 Discord Webhook 地址，发送内容由你选择。这里可以发送两类内容： <b>状态卡片</b>
		Warcon 创建一次并持续更新的状态卡片（在线人数、地图、各阵营分数和玩家名单；
		<b>在 Discord 中置顶</b>
		可将其置顶），以及
		<b>误杀队友</b
		>击杀事件上报的误杀队友消息。若希望分开显示，请分别创建频道。封禁、踢出、规则操作和登录等管理员操作的镜像通知在
		<a class="link" href={orgPage}>组织页面</a>.
	</p>
	{#if data.owner && !data.https}
		<div class="callout mb-3 border-warn/30 bg-warn/12">
			<b>卡片将不附带图片。</b> Discord 只通过 HTTPS 获取地图图片和图标；此面板尚未使用 HTTPS，因此卡片不会显示图片，其余内容仍可正常使用。
		</div>
	{/if}
	{#if !data.owner}
		<p class="note">
			只有以下组织的所有者： {data.server.orgName} 只有组织所有者能连接 Discord 频道，因为持有 Webhook
			地址的人都能向该频道发消息。
		</p>
	{:else}
		{#each data.channels as w (w.id)}
			<div class="kv items-start">
				<div class="min-w-0">
					<div>
						{w.label}
						{#if !w.enabled}<Badge class="ml-1">已暂停</Badge>{/if}
						{#if w.lastError}<Badge tone="err" class="ml-1">异常</Badge
							>{:else if w.statusSentAt}<Badge tone="ok" class="ml-1">实时</Badge>{/if}
					</div>
					<div class="truncate font-mono text-[11px] text-mist-600">{w.urlHint}</div>
					<div class="text-[12px] text-mist-400">
						{carries(w)}{#if w.statusEnabled}
							· {linksOf(w)}{/if}
						{#if !w.serverIds}· 组织中的每台服务器{:else if w.serverIds.length > 1}· 本服及 {w
								.serverIds.length - 1} 台其他服务器{w.serverIds.length === 2 ? '' : 's'}{/if}
						{#if w.lastError}<div class="text-danger">{w.lastError}</div>
							<div>{hintFor(w.lastError)}</div>{:else if w.statusSentAt}· 已更新 {fmtTime(
								w.statusSentAt
							)}{/if}
					</div>
				</div>
				{#if ownHere(w)}
					<button class="btn btn-sm shrink-0" onclick={() => openEdit(w)} disabled={busy}
						>编辑</button
					>
				{:else}
					<a class="btn btn-sm shrink-0 btn-ghost" href={orgPage}>在组织页面编辑</a>
				{/if}
			</div>
		{:else}
			<p class="mb-3 text-[13px] text-mist-600">当前没有频道发送本服状态卡片或误杀队友消息。</p>
		{/each}

		<form
			class="mt-4 space-y-3 border-t border-white/8 pt-4"
			onsubmit={(e) => {
				e.preventDefault();
				void add();
			}}
		>
			<span class="field-label">连接频道</span>
			<div class="grid gap-3 sm:grid-cols-[1fr_2fr]">
				<label class="block"
					><span class="field-label">标签</span><input
						id="discord-label"
						class="input"
						type="text"
						bind:value={label}
						placeholder="例如 #eu-1-status"
						maxlength="60"
					/></label
				>
				<label class="block"
					><span class="field-label">Webhook 地址</span><input
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
			<ChannelFields
				name="carry"
				bind:carry
				bind:style={card.style}
				bind:interval={card.interval}
				bind:linkStatus={card.linkStatus}
				bind:linkLeaderboard={card.linkLeaderboard}
				bind:linkPanel={card.linkPanel}
				{features}
			/>
			<p class="note">
				在 Discord 中打开频道设置 → 集成 → Webhook → 新建
				Webhook，复制地址并粘贴到这里。地址会加密保存，之后不会再次显示。显示图片需要面板支持
				HTTPS。卡片链接的公开页面可在下方“公开页面”中开启。
			</p>
			<div class="flex justify-end">
				<button type="submit" class="btn btn-primary" disabled={busy}>连接频道</button>
			</div>
		</form>
	{/if}
</div>

{#if editing}
	{@const e = editing}
	<Modal title="Edit {e.w.label}" onclose={() => (editing = null)}>
		<form
			class="space-y-3"
			onsubmit={(ev) => {
				ev.preventDefault();
				void save();
			}}
		>
			<label class="block"
				><span class="field-label">标签</span><input
					class="input"
					type="text"
					bind:value={e.label}
					maxlength="60"
				/></label
			>
			<ChannelFields
				name="edit-carry"
				bind:carry={e.carry}
				bind:style={e.card.style}
				bind:interval={e.card.interval}
				bind:linkStatus={e.card.linkStatus}
				bind:linkLeaderboard={e.card.linkLeaderboard}
				bind:linkPanel={e.card.linkPanel}
				{features}
			/>
			<div class="flex flex-wrap items-center gap-2 pt-2">
				{#if e.w.statusEnabled}
					<button
						type="button"
						class="btn"
						onclick={() => testCard(e.w)}
						disabled={busy || !e.w.enabled}>测试卡片</button
					>
				{/if}
				<button type="button" class="btn" onclick={() => toggle(e.w)} disabled={busy}
					>{e.w.enabled ? 'Pause' : 'Enable'}</button
				>
				<span class="ml-auto inline-flex gap-2">
					<button type="button" class="btn btn-danger" onclick={() => remove(e.w)} disabled={busy}
						>断开连接</button
					>
					<button type="submit" class="btn btn-primary" disabled={busy}>保存</button>
				</span>
			</div>
		</form>
	</Modal>
{/if}
