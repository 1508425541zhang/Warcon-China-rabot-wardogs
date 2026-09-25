<script lang="ts">
	import { api, rconGet, rconPost, errorMessage, ApiError } from '$lib/api';
	import { can } from '$lib/capabilities';
	import { toast } from '$lib/toast.svelte';
	import { confirmDialog } from '$lib/confirm.svelte';
	import ConfigForm from '$lib/components/ConfigForm.svelte';
	import TickReward from '$lib/components/TickReward.svelte';
	import { sponsor as banners, loadSponsor } from '$lib/sponsor.svelte';
	import { setScalarInText } from '$lib/config-doc';
	import { lockedKeys, S_SESSION } from '$lib/config-fields';
	import { fmtTime } from '$lib/format';
	import type { ConfigDoc, ConfigResult, Status } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let admin = $derived(can(data.server.caps, 'config.apply'));

	let tick = $state(24);
	let tickMin = $state(18);
	let tickMax = $state(30);
	let tickKnown = $state(false);
	let sponsor = $state('');
	let sponsorShown = $derived(banners[id] ?? '');
	let doc = $state<ConfigDoc | null>(null);
	let text = $state('');
	let force = $state(false);
	let docError = $state('');
	let result = $state<ConfigResult | null>(null);
	let failure = $state('');
	let lineErrors = $state<{ line?: number; section?: string; key?: string; message?: string }[]>(
		[]
	);
	let busy = $state(false);
	let mode = $state<'form' | 'raw'>('form');

	let readOnly = $derived(!admin || !doc || !doc.writable);

	// The kill feed: the game posts every kill to Warcon once [WDServerFeed] Url and Token are set
	// in its config. The token is the server's identity on that route; org owners mint and see it.
	interface FeedSetup {
		configured: boolean;
		url: string;
		token: string;
		feedAt: string | null;
	}
	let feed = $state<FeedSetup | null>(null);
	let feedIsLoopback = $derived.by(() => {
		try {
			return ['127.0.0.1', 'localhost', '[::1]'].includes(new URL(feed?.url ?? '').hostname);
		} catch {
			return false;
		}
	});
	let feedBusy = $state(false);
	let feedPath = $derived(`/api/servers/${encodeURIComponent(id)}/feed`);
	async function loadFeed() {
		try {
			feed = await api<FeedSetup>('GET', feedPath);
		} catch {
			feed = null;
		}
	}
	async function feedAction(fn: () => Promise<unknown>, done: string) {
		feedBusy = true;
		try {
			await fn();
			toast(done, 'ok');
			await loadFeed();
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			feedBusy = false;
		}
	}
	/** One click: mint the token, write both keys into the config document, apply. */
	async function configureFeed() {
		const writable = !readOnly;
		feedBusy = true;
		try {
			feed = await api<FeedSetup>('POST', feedPath);
		} catch (err) {
			toast(errorMessage(err), 'err');
			feedBusy = false;
			return;
		}
		feedBusy = false;
		if (writable) await writeFeedConfig();
		else
			toast(
				'令牌已创建。这里无法写入配置文件，请在服务器主机上手动设置 [WDServerFeed] 的 Url 和 Token。',
				'ok'
			);
		await loadFeed();
	}
	async function rotateFeed() {
		if (
			!(await confirmDialog(
				'确定更换令牌？在更新配置并重启游戏服务器前，旧令牌仍会用于发送数据，但请求会被拒绝。',
				{ okLabel: 'Replace', danger: true }
			))
		)
			return;
		await feedAction(() => api('POST', feedPath), '令牌已更换，请重新写入配置文件。');
	}
	async function disableFeed() {
		if (
			!(await confirmDialog(
				'确定关闭击杀事件源？已存储的记录会保留；在配置新令牌前，游戏服务器发送的数据会被拒绝。',
				{ okLabel: '关闭', danger: true }
			))
		)
			return;
		await feedAction(() => api('DELETE', feedPath), '击杀事件源已关闭。');
	}
	async function writeFeedConfig() {
		if (!feed?.token || !doc) return;
		if (
			dirty &&
			!(await confirmDialog('还有其他未应用的配置修改。是否与击杀事件源设置一起应用到服务器？'))
		)
			return;
		text = setScalarInText(text, 'WDServerFeed', 'Url', feed.url);
		text = setScalarInText(text, 'WDServerFeed', 'Token', feed.token);
		await runConfig('configApply');
		if (!failure) toast('击杀事件源已配置，游戏服务器下次重启后开始发送数据。', 'ok');
	}
	async function copyFeed(value: string, what: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast(`${what} copied.`, 'ok');
		} catch {
			window.prompt(`Copy the ${what.toLowerCase()}:`, value);
		}
	}
	const feedAge = (iso: string) => {
		const s = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000));
		return s < 90 ? `${s} 秒前` : s < 5400 ? `${Math.round(s / 60)} 分钟前` : fmtTime(iso);
	};
	$effect(() => {
		void loadFeed();
	});
	let pinned = $derived(doc ? lockedKeys(doc.sections) : []);
	let dirty = $derived(!!doc && text !== doc.text);

	async function loadDoc(opts: { keepResult?: boolean } = {}) {
		if (!opts.keepResult) {
			result = null;
			failure = '';
			lineErrors = [];
		}
		try {
			doc = await rconGet<ConfigDoc>(id, 'config');
			text = doc.text;
			docError = '';
		} catch (err) {
			doc = null;
			text = '';
			docError = errorMessage(err);
		}
	}
	async function reload() {
		if (dirty && !(await confirmDialog('确定从服务器重新加载并放弃尚未应用的修改？'))) return;
		await loadDoc();
	}
	function discard() {
		if (doc) text = doc.text;
		failure = '';
		lineErrors = [];
	}
	async function copyText() {
		try {
			await navigator.clipboard.writeText(text);
			toast('配置已复制到剪贴板。', 'ok');
		} catch {
			toast('复制失败，请选中原始配置文件手动复制。', 'err');
		}
	}
	function download() {
		const blob = new Blob([text], { type: 'text/plain' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = `ServerSettings-${data.server.name.replace(/[^\w.-]+/g, '_')}.ini`;
		a.click();
		setTimeout(() => URL.revokeObjectURL(url), 1000);
	}
	// The banner is shared with the server header; a forced read follows an apply.
	async function loadBanner(force = false) {
		await loadSponsor(id, force);
		sponsor = banners[id] ?? '';
	}
	async function loadTick() {
		try {
			const st = await rconGet<Status>(id, 'status');
			if (st.scoreTickMin) tickMin = st.scoreTickMin;
			if (st.scoreTickMax) tickMax = st.scoreTickMax;
			if (st.scoreTick !== null) {
				tick = st.scoreTick;
				tickKnown = true;
			}
		} catch (err) {
			toast(errorMessage(err), 'err');
		}
	}
	$effect(() => {
		void Promise.all([loadTick(), loadBanner(), loadDoc()]);
	});

	async function saveTick() {
		try {
			const r = await rconPost<{ message?: string }>(id, 'settings', { scoreTick: tick });
			toast(r?.message || '计分周期已保存。', 'ok');
		} catch (err) {
			toast(errorMessage(err), 'err');
		}
	}
	// The game has no live route for the banner (real listeners answer PUT /v1/sponsor with
	// "PUT is not supported"), so, like the official console, this writes ServerImageURL into the
	// config document and applies it. Any other unapplied form edits go with it, as they would
	// from the Apply button below.
	async function saveSponsor() {
		if (readOnly) return;
		if (
			dirty &&
			!(await confirmDialog('还有其他未应用的配置修改。是否与赞助图片一起应用到服务器？'))
		)
			return;
		text = setScalarInText(text, S_SESSION, 'ServerImageURL', sponsor.trim());
		await runConfig('configApply');
		// On a refusal (for example a host that is not on the server's image allow-list) runConfig
		// has already shown the reason; keep what was typed so it can be corrected.
		if (failure) return;
		await loadBanner(true);
	}
	async function runConfig(action: 'configValidate' | 'configApply') {
		busy = true;
		result = null;
		failure = '';
		lineErrors = [];
		try {
			const r = await rconPost<ConfigResult>(
				id,
				action,
				action === 'configApply' ? { text, revision: doc?.revision, force } : { text }
			);
			if (r.conflict) {
				failure = `${r.errorMessage || '加载后服务器上的配置已发生变化。'} Reload to see the current version, or tick Force to overwrite.`;
				return;
			}
			if (!r.ok) {
				failure = r.errorMessage || '服务器拒绝了配置修改。';
				lineErrors = r.errors || [];
				return;
			}
			result = r;
			if (action === 'configApply' && doc) {
				doc.revision = r.revision;
				toast(`配置已应用（版本 ${r.revision}）。`, 'ok');
				// Re-read so the change marks and revision reflect what the server actually kept.
				await loadDoc({ keepResult: true });
			} else {
				toast('配置有效。', 'ok');
			}
		} catch (err) {
			failure = errorMessage(err);
			const body =
				err instanceof ApiError
					? (err.data as { error?: { body?: { errors?: { line?: number; message?: string }[] } } })
							?.error?.body
					: null;
			lineErrors = body?.errors || [];
		} finally {
			busy = false;
			if (failure) toast(lineErrors[0]?.message || failure, 'err');
		}
	}
	const PIP: Record<string, string> = {
		applied: 'bg-ok/15 text-ok',
		'next-match': 'bg-info/15 text-info',
		'next-restart': 'bg-warn/15 text-warn',
		pending: 'bg-white/10 text-mist-400',
		shadowed: 'bg-override/15 text-override'
	};
</script>

<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
	<div class="panel">
		<span class="label-sm">得分周期（KOTH ScorePeriod）</span>
		<div class="flex items-center gap-3">
			<span class="text-mist-400">{tickMin}s</span>
			<input
				type="range"
				class="grow"
				min={tickMin}
				max={tickMax}
				step="1"
				bind:value={tick}
				disabled={!admin || !data.features.liveSettings}
				onchange={saveTick}
			/>
			<span class="text-mist-400">{tickMax}s</span>
			<output class="w-10 font-mono">{tickKnown ? `${tick}s` : '—'}</output>
		</div>
		<TickReward seconds={tickKnown ? tick : null} class="mt-2" />
		<p class="note">
			{#if data.features.liveSettings}通过实时接口（PATCH /v1/settings）修改。{:else}当前服务器版本不支持实时设置接口，请在下方配置文件中修改
				ScorePeriod。{/if}
			得分周期越短，现金倍率越低；游戏会把数值限制在允许范围内。
		</p>
	</div>
	<div class="panel">
		<span class="label-sm">赞助图片</span>
		<form
			class="join w-full"
			onsubmit={(e) => {
				e.preventDefault();
				void saveSponsor();
			}}
		>
			<input
				class="input"
				type="url"
				placeholder="https://…/banner.png (1024×256)"
				bind:value={sponsor}
				disabled={readOnly}
			/>
			<button class="btn btn-primary" type="submit" disabled={readOnly || busy}>应用</button>
		</form>
		{#if sponsorShown}<img
				src={sponsorShown}
				alt=""
				class="mt-3 max-w-full rounded-card border border-black"
				referrerpolicy="no-referrer"
			/>{/if}
		<p class="note">
			服务器浏览器中显示的横幅。请填写服务器图片白名单域名（catbox.moe、imgbb.com、postimg.cc）上的
			1024×256 PNG/JPEG 直链。保存时写入配置文件的
			ServerImageURL；服务器下载并验证图片后才会对外展示，因此在验证完成前会显示为待生效。
			{#if docError}该服务器没有配置文件，无法在此更换横幅。{:else if doc && !doc.writable}配置文件为只读状态，无法在此更换横幅。{/if}
		</p>
	</div>
</div>

<div class="mt-4 panel">
	<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
		<span class="label-sm mb-0">击杀事件</span>
		{#if feed}
			{#if feed.configured}
				<span class="badge bg-ok/15 text-ok">开启</span>
				<span class="text-[12.5px] text-mist-600"
					>{feed.feedAt ? `last batch ${feedAge(feed.feedAt)}` : '尚未收到任何批次'}</span
				>
			{:else}
				<span class="badge">关闭</span>
			{/if}
		{/if}
		{#if data.server.manager && feed}
			<span class="ml-auto inline-flex flex-wrap gap-1.5">
				{#if feed.configured}
					<button class="btn btn-sm" disabled={feedBusy} onclick={rotateFeed}>更换令牌</button>
					<button class="btn btn-sm btn-danger" disabled={feedBusy} onclick={disableFeed}
						>关闭</button
					>
				{:else}
					<button
						class="btn btn-sm btn-primary"
						disabled={feedBusy || busy || !doc || (feedIsLoopback && !data.server.demo)}
						onclick={configureFeed}>配置</button
					>
				{/if}
			</span>
		{/if}
	</div>
	{#if feedIsLoopback && !data.server.demo}
		<p class="mb-3 text-sm text-warn">
			当前回传地址是本机地址，远程游戏服务器无法向它发送击杀事件。请先提供游戏服务器可访问的公网
			HTTPS 地址，再配置 Kill Feed。
		</p>
	{/if}
	{#if feed?.configured && feed.token}
		<div class="grid grid-cols-1 gap-3 md:grid-cols-[auto_1fr]">
			<span class="text-[13px] text-mist-400 md:pt-1.5">地址</span>
			<div class="flex items-center gap-2">
				<code
					class="min-w-0 grow truncate rounded-ctl border border-black bg-ink-950 px-2.5 py-1.5 font-mono text-[12.5px]"
					>{feed.url}</code
				>
				<button class="btn btn-sm" onclick={() => copyFeed(feed!.url, 'URL')}>复制</button>
				<span class="text-[12.5px] text-mist-600">游戏会自行添加 /api/ingest/events</span>
			</div>
			<span class="text-[13px] text-mist-400 md:pt-1.5">令牌</span>
			<div class="flex items-center gap-2">
				<code
					class="min-w-0 grow truncate rounded-ctl border border-black bg-ink-950 px-2.5 py-1.5 font-mono text-[12.5px]"
					>{feed.token}</code
				>
				<button class="btn btn-sm" onclick={() => copyFeed(feed!.token, 'Token')}>复制</button>
			</div>
		</div>
		<div class="mt-3 flex flex-wrap items-center gap-2">
			<button class="btn btn-sm" disabled={readOnly || busy} onclick={writeFeedConfig}
				>再次写入配置</button
			>
			<span class="text-[12.5px] text-mist-600"
				>下方配置文件已包含两个必需键；更换令牌或在主机上直接编辑文件后，请重新写入。</span
			>
		</div>
	{/if}
	<p class="note">
		与 <span class="chip">[WDServerFeed]</span> 配置完成后，游戏会在击杀发生后一两秒向 Warcon
		上报击杀者、受害者、武器、距离和爆头信息，用于概览页的击杀事件、分析与玩家档案中的战斗统计，以及误杀规则。
		{#if feed && !feed.configured}“配置”会把接收地址和令牌写入配置文件；游戏在下次重启时读取（可等待自身每
			24 小时的重启或手动重启）。{:else if feed && !data.server.manager}令牌由组织所有者保管。{/if}
	</p>
</div>

<div class="mt-4 panel">
	<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
		<span class="label-sm mb-0">配置文件（ServerSettings.ini）</span>
		<span class="text-[12.5px] text-mist-400"
			>修订版 <span class="font-mono">{doc?.revision || (doc ? '(none)' : '—')}</span>{#if dirty}
				· <span class="text-accent">未应用的修改</span>{/if}</span
		>
		<span class="join ml-auto">
			<button
				class="btn btn-sm {mode === 'form' ? 'btn-primary' : ''}"
				onclick={() => (mode = 'form')}>表单</button
			>
			<button
				class="btn btn-sm {mode === 'raw' ? 'btn-primary' : ''}"
				onclick={() => (mode = 'raw')}>原始文件</button
			>
		</span>
	</div>
	<div class="mb-4 flex flex-wrap items-center gap-2">
		<div class="join">
			<button class="btn btn-sm" onclick={reload}>重新加载</button>
			<button class="btn btn-sm" disabled={!dirty} onclick={discard}>放弃修改</button>
			<button
				class="btn btn-sm"
				disabled={!admin || !doc || busy}
				onclick={() => runConfig('configValidate')}>验证</button
			>
			<button
				class="btn btn-sm btn-primary"
				disabled={readOnly || busy}
				onclick={() => runConfig('configApply')}>应用到服务器</button
			>
		</div>
		<div class="join">
			<button class="btn btn-sm" disabled={!doc} onclick={copyText}>复制</button>
			<button class="btn btn-sm" disabled={!doc} onclick={download}>下载 .ini</button>
		</div>
		<label class="inline-flex items-center gap-2 text-[12.5px]"
			><input type="checkbox" bind:checked={force} /> 强制保存（忽略版本冲突）</label
		>
	</div>
	{#if failure}
		<div class="callout mb-4 border-danger/30 bg-danger/12">
			<div>{failure}</div>
			{#each lineErrors as e, i (i)}
				<div class="mt-2 flex flex-wrap items-center gap-2 text-[13px]">
					<span class="pip {PIP['next-restart']}"
						>{e.line !== undefined ? `line ${e.line}` : 'rejected'}</span
					>{#if e.key}<span class="font-mono">{e.key}</span>{/if}<span
						>{e.message || String(e)}</span
					>
				</div>
			{/each}
		</div>
	{/if}
	{#if mode === 'form' && doc}
		<ConfigForm
			bind:text
			serverText={doc.text}
			sections={doc.sections}
			shadowed={result?.shadowed ?? []}
			disabled={readOnly}
			tickRange={tickKnown ? { min: tickMin, max: tickMax } : null}
			liveRoutes={data.features.liveSettings}
		/>
		<p class="note mt-4">
			表单按行修改配置文件，未列出的键（地图轮换、封禁与预留位名单、RCON
			区块）及注释都会原样保留。切换到“原始文件”可查看或编辑完整内容。
		</p>
	{:else}
		<textarea
			class="min-h-[420px] input font-mono text-[12.5px] leading-relaxed pointer-coarse:text-[16px]"
			rows="26"
			spellcheck="false"
			bind:value={text}
			readonly={readOnly}></textarea>
	{/if}

	{#if docError}
		<div class="mt-3 callout mb-0">
			该服务器没有配置文件（{docError}）。较旧的 WDRCON 版本只提供上方的实时设置。
		</div>
	{:else if doc && !doc.writable}
		<div class="mt-3 callout mb-0">
			该服务器报告配置文件为只读（未启用 -StandaloneConfig），这里的修改无法应用。
		</div>
	{/if}
	{#if pinned.length}
		<div class="mt-3 callout mb-0">
			以下值由服务器启动参数固定，只能查看：
			<ul class="mt-1 list-disc pl-5">
				{#each pinned as k (k.section + '|' + k.key)}
					<li>
						<span class="font-mono">{k.key}</span>{#if k.lockedBy}&nbsp;(-{k.lockedBy}){/if}:
						{k.description}
					</li>
				{/each}
			</ul>
		</div>
	{/if}
	{#each doc?.warnings ?? [] as w, i (i)}
		<div class="mt-3 callout mb-0">
			{typeof w === 'object' && w && 'message' in w
				? String((w as { message: unknown }).message)
				: String(w)}
		</div>
	{/each}
	{#if result}
		<div class="mt-3 space-y-1.5">
			{#each result.outcomes as o (o.section)}
				<div class="flex flex-wrap items-center gap-2 text-[13px]">
					<span class="pip {PIP[o.state] || PIP.pending}">{o.state}</span><span class="font-mono"
						>{o.section}</span
					>{#if o.detail}<span class="text-mist-400">{o.detail}</span>{/if}
				</div>
			{/each}
			{#each result.shadowed as s (s.section + s.key)}
				<div class="flex flex-wrap items-center gap-2 text-[13px]">
					<span class="pip {PIP.shadowed}">覆盖修改</span><span class="font-mono"
						>{s.section} {s.key}</span
					><span class="text-mist-400">声明值 {s.declared}，生效值 {s.effective}</span>
				</div>
			{/each}
			{#each result.warnings as w, i (i)}
				<div class="flex flex-wrap items-center gap-2 text-[13px]">
					<span class="pip {PIP.pending}">警告</span><span
						>{typeof w === 'object' && w && 'message' in w
							? String((w as { message: unknown }).message)
							: String(w)}</span
					>
				</div>
			{/each}
			{#if result.timingsMs?.total !== undefined}<div class="font-mono text-[12px] text-mist-600">
					{result.timingsMs.total} ms
				</div>{/if}
		</div>
	{/if}
	<p class="note">
		“应用”会提交完整文件。服务器会整体验证，并按区块告知修改是立即生效、下场比赛生效，还是需要重启。服务器必须使用
		-StandaloneConfig=&lt;path&gt; 启动，否则这里的配置文件为只读。
	</p>
</div>
