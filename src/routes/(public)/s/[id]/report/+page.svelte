<script lang="ts">
	import { api, errorMessage } from '$lib/api';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const categories = [
		{ label: '作弊疑似', hint: '瞄准、透视或异常信息优势' },
		{ label: '破坏对局', hint: '故意送分、恶意卡位或团队破坏' },
		{ label: '恶意交流', hint: '骚扰、威胁、仇恨或持续辱骂' },
		{ label: '利用漏洞', hint: '利用地图、机制或已知漏洞获利' },
		{ label: '其他', hint: '需要管理员进一步判断的问题' }
	] as const;

	let target = $state('');
	let detail = $state('');
	let category = $state<(typeof categories)[number]['label']>('作弊疑似');
	let busy = $state(false);
	let success = $state(false);
	let failure = $state('');
	let reportId = $state<number | null>(null);
	let targetSteamId = $state('');

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		failure = '';
		try {
			const reason = '【' + category + '】 ' + detail.trim();
			const result = await api<{
				ok: true;
				report: { id: number; targetSteamId: string };
			}>('POST', '/api/reports', {
				serverId: data.heading.id,
				target: target.trim(),
				reason
			});
			reportId = result.report.id;
			targetSteamId = result.report.targetSteamId;
			success = true;
			target = '';
			detail = '';
		} catch (err) {
			failure = errorMessage(err);
		} finally {
			busy = false;
		}
	}

	function reset() {
		success = false;
		reportId = null;
		targetSteamId = '';
		failure = '';
		category = '作弊疑似';
	}
</script>

<svelte:head>
	<title>战斗 Report · {data.heading.name}</title>
	<meta
		name="description"
		content="战斗 Report 社区举报入口：提交玩家、分类与对局上下文，系统保存相关证据供管理员人工复核。"
	/>
</svelte:head>

<section class="battle-report" aria-labelledby="battle-report-title">
	<div class="ambient ambient-a"></div>
	<div class="ambient ambient-b"></div>
	<div class="noise"></div>

	<div class="hero-grid">
		<div class="hero-copy">
			<div class="eyebrow">
				<span class="live-dot"></span>
				COMMUNITY INTEGRITY / 战斗社区
			</div>

			<h1 id="battle-report-title">
				<span class="battle-word">战斗</span>
				<span class="report-word">REPORT</span>
			</h1>

			<p class="hero-lede">
				把一场有争议的对局变成一条可复核的记录。选择玩家、说明原因，系统会把举报写入数据库，
				并关联时间窗内的服务器证据，交给管理员判断。
			</p>

			<div class="principles" aria-label="举报原则">
				<div class="principle">
					<span class="principle-number">01</span>
					<div>
						<strong>定位目标</strong>
						<span>玩家名或 SteamID64</span>
					</div>
				</div>
				<div class="principle">
					<span class="principle-number">02</span>
					<div>
						<strong>记录上下文</strong>
						<span>分类 + 事实描述</span>
					</div>
				</div>
				<div class="principle">
					<span class="principle-number">03</span>
					<div>
						<strong>证据入库</strong>
						<span>关联服务器事件供复核</span>
					</div>
				</div>
			</div>

			<div class="integrity-note">
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path
						d="M12 3 5 6v5c0 4.9 2.8 8.4 7 10 4.2-1.6 7-5.1 7-10V6l-7-3Zm0 3.1 4 1.7V11c0 3.2-1.5 5.7-4 7-2.5-1.3-4-3.8-4-7V7.8l4-1.7Z"
					/>
				</svg>
				<span>
					<strong>举报不是定罪。</strong>
					单靠举报不会自动封禁玩家，最终结论以管理员复核和服务器证据为准。
				</span>
			</div>
		</div>

		<div class="visual-shell" aria-label="动态战术证据雷达">
			<div class="hud-topline">
				<span>EVIDENCE LINK</span>
				<span class="hud-online">● ONLINE</span>
			</div>

			<div class="radar">
				<div class="radar-ring ring-a"></div>
				<div class="radar-ring ring-b"></div>
				<div class="radar-ring ring-c"></div>
				<div class="radar-cross cross-x"></div>
				<div class="radar-cross cross-y"></div>
				<div class="sweep"></div>
				<div class="blip blip-a"><span></span></div>
				<div class="blip blip-b"><span></span></div>
				<div class="blip blip-c"><span></span></div>

				<svg class="reticle" viewBox="0 0 160 160" aria-hidden="true">
					<g class="reticle-spin">
						<path d="M80 8v22M80 130v22M8 80h22M130 80h22" />
						<path d="M30 30 45 45M115 115l15 15M130 30l-15 15M45 115l-15 15" />
					</g>
					<circle cx="80" cy="80" r="18" />
					<circle cx="80" cy="80" r="4" class="reticle-core" />
				</svg>
			</div>

			<div class="telemetry">
				<div>
					<span>证据窗口</span>
					<strong>±180s</strong>
				</div>
				<div>
					<span>事件上限</span>
					<strong>1000</strong>
				</div>
				<div>
					<span>处理方式</span>
					<strong>人工复核</strong>
				</div>
			</div>

			<div class="data-stream" aria-hidden="true">
				<span>CAPTURE / PLAYER EVENT / TIMESTAMP / SERVER</span>
				<span>TRACE / MATCH CONTEXT / EVIDENCE / REVIEW</span>
			</div>
		</div>
	</div>

	<div class="report-layout">
		<aside class="case-brief">
			<p class="section-kicker">CASE PROTOCOL</p>
			<h2>提交前，尽量写清“发生了什么”</h2>
			<p>
				高质量举报应该描述可核查的行为，而不是只写“他开挂”。例如：出现在哪一轮、发生了什么异常、
				是否反复发生。Steam 和 FACEIT 的举报流程同样强调选择具体违规类型并补充上下文或证据。
			</p>

			<div class="brief-list">
				<div>
					<span class="brief-index">A</span>
					<p><strong>保持客观</strong><br />描述行为、时间和场景，避免人身信息与猜测。</p>
				</div>
				<div>
					<span class="brief-index">B</span>
					<p><strong>目标要准确</strong><br />同名玩家较多时，优先使用完整 SteamID64。</p>
				</div>
				<div>
					<span class="brief-index">C</span>
					<p><strong>重复举报会限流</strong><br />系统会抑制短时间内对同一玩家的重复提交。</p>
				</div>
			</div>
		</aside>

		<div class="form-card">
			<div class="form-card-head">
				<div>
					<p class="section-kicker">NEW REPORT</p>
					<h2>新建社区举报</h2>
				</div>
				<div class="case-chip">WAR / CASE</div>
			</div>

			{#if success}
				<div class="success-state" role="status">
					<div class="success-mark">
						<svg viewBox="0 0 24 24" aria-hidden="true">
							<path d="m6.5 12.5 3.3 3.3L17.8 8" />
						</svg>
					</div>
					<p class="section-kicker">REPORT STORED</p>
					<h3>举报已进入审核队列</h3>
					<p>
						系统已经保存这次举报，并开始关联时间窗内的服务器事件。管理员看到的是待核查材料，
						不是自动生成的作弊结论。
					</p>
					<div class="receipt">
						<div>
							<span>举报编号</span>
							<strong>#{reportId}</strong>
						</div>
						<div>
							<span>目标 SteamID</span>
							<strong>{targetSteamId}</strong>
						</div>
					</div>
					<button class="secondary-action" type="button" onclick={reset}>继续提交另一条举报</button>
				</div>
			{:else if !data.signedIn}
				<div class="auth-state">
					<div class="auth-icon" aria-hidden="true">
						<svg viewBox="0 0 24 24">
							<path d="M7 10V8a5 5 0 0 1 10 0v2M6 10h12v10H6z" />
						</svg>
					</div>
					<h3>登录后才能提交</h3>
					<p>
						为了减少匿名滥用，举报需要登录账号并绑定已验证的 Steam。登录后系统会继续执行重复举报限制和审计记录。
					</p>
					<a class="primary-action" href="/sign-in">
						登录并开始举报
						<span aria-hidden="true">↗</span>
					</a>
					<a class="account-link" href="/account">已有账号？前往账号页检查 Steam 绑定</a>
				</div>
			{:else}
				<form onsubmit={submit}>
					<div class="field-block">
						<div class="field-heading">
							<label for="report-target">举报玩家</label>
							<span>PLAYER / STEAM</span>
						</div>
						<div class="input-wrap">
							<svg viewBox="0 0 24 24" aria-hidden="true">
								<path d="M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm-7 8a7 7 0 0 1 14 0" />
							</svg>
							<input
								id="report-target"
								bind:value={target}
								maxlength="200"
								placeholder="玩家名 或 17 位 SteamID64"
								autocomplete="off"
								required
							/>
						</div>
						<p class="field-help">玩家必须在该服务器最近约 15 分钟内出现过。</p>
					</div>

					<fieldset class="field-block">
						<div class="field-heading">
							<legend>举报分类</legend>
							<span>REASON TYPE</span>
						</div>
						<div class="category-grid">
							{#each categories as item}
								<button
									type="button"
									class:category-active={category === item.label}
									aria-pressed={category === item.label}
									onclick={() => (category = item.label)}
								>
									<strong>{item.label}</strong>
									<span>{item.hint}</span>
								</button>
							{/each}
						</div>
					</fieldset>

					<div class="field-block">
						<div class="field-heading">
							<label for="report-detail">发生了什么</label>
							<span>{detail.length}/260</span>
						</div>
						<textarea
							id="report-detail"
							bind:value={detail}
							minlength="3"
							maxlength="260"
							rows="6"
							placeholder="示例：第 4 回合约 02:10，目标在没有视野的情况下连续预瞄两名玩家；第 6 回合出现了相同情况。请复核对应时间段。"
							required
						></textarea>
						<p class="field-help">不要填写住址、电话等现实身份信息；只提交与本次游戏行为有关的内容。</p>
					</div>

					<div class="submit-row">
						<div class="storage-note">
							<span class="storage-pulse"></span>
							<span>提交后写入社区完整性数据库，并保存相关事件用于复核</span>
						</div>
						<button class="primary-action submit-action" type="submit" disabled={busy}>
							{busy ? '正在记录…' : '提交举报'}
							<span aria-hidden="true">{busy ? '•••' : '→'}</span>
						</button>
					</div>
				</form>
			{/if}

			{#if failure}
				<div role="alert" class="error-state">
					<strong>提交失败</strong>
					<span>{failure}</span>
				</div>
			{/if}
		</div>
	</div>
</section>

<style>
	.battle-report {
		--acid: #c8ff37;
		--acid-soft: rgba(200, 255, 55, 0.16);
		--cyan: #67e8f9;
		--red: #ff5d62;
		--ink: #070a0c;
		--panel: rgba(14, 18, 20, 0.82);
		position: relative;
		isolation: isolate;
		overflow: hidden;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 22px;
		background:
			linear-gradient(180deg, rgba(255, 255, 255, 0.025), transparent 26%),
			radial-gradient(circle at 12% 5%, rgba(200, 255, 55, 0.075), transparent 32%),
			radial-gradient(circle at 86% 18%, rgba(103, 232, 249, 0.06), transparent 30%),
			#080b0d;
		box-shadow: 0 30px 80px rgba(0, 0, 0, 0.34);
	}

	.battle-report::before {
		position: absolute;
		inset: 0;
		z-index: -1;
		content: '';
		opacity: 0.13;
		background-image:
			linear-gradient(rgba(255, 255, 255, 0.11) 1px, transparent 1px),
			linear-gradient(90deg, rgba(255, 255, 255, 0.11) 1px, transparent 1px);
		background-size: 44px 44px;
		mask-image: linear-gradient(to bottom, black, transparent 78%);
	}

	.ambient {
		position: absolute;
		z-index: -1;
		width: 30rem;
		height: 30rem;
		border-radius: 999px;
		filter: blur(90px);
		opacity: 0.14;
		pointer-events: none;
		animation: drift 13s ease-in-out infinite alternate;
	}

	.ambient-a {
		top: -14rem;
		left: -9rem;
		background: var(--acid);
	}

	.ambient-b {
		top: 6rem;
		right: -15rem;
		background: var(--cyan);
		animation-delay: -5s;
	}

	.noise {
		position: absolute;
		inset: 0;
		z-index: 8;
		pointer-events: none;
		opacity: 0.035;
		background:
			repeating-linear-gradient(
				0deg,
				rgba(255, 255, 255, 0.7) 0,
				rgba(255, 255, 255, 0.7) 1px,
				transparent 1px,
				transparent 3px
			);
		mix-blend-mode: overlay;
	}

	.hero-grid {
		display: grid;
		grid-template-columns: minmax(0, 1.07fr) minmax(320px, 0.93fr);
		gap: 2.2rem;
		padding: clamp(2rem, 4vw, 4.7rem);
		padding-bottom: clamp(1.5rem, 3vw, 3rem);
	}

	.hero-copy {
		display: flex;
		flex-direction: column;
		justify-content: center;
	}

	.eyebrow,
	.section-kicker {
		margin: 0;
		font-family: 'JetBrains Mono Variable', 'JetBrains Mono', monospace;
		font-size: 0.69rem;
		font-weight: 700;
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: rgba(255, 255, 255, 0.52);
	}

	.eyebrow {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		color: rgba(200, 255, 55, 0.76);
	}

	.live-dot,
	.storage-pulse {
		display: inline-block;
		width: 0.45rem;
		height: 0.45rem;
		border-radius: 999px;
		background: var(--acid);
		box-shadow: 0 0 0 0 rgba(200, 255, 55, 0.4);
		animation: pulse 1.8s ease-out infinite;
	}

	h1 {
		display: flex;
		flex-wrap: wrap;
		gap: 0.18em;
		align-items: baseline;
		margin: 1.2rem 0 1rem;
		font-family: 'Barlow Condensed', 'Arial Narrow', sans-serif;
		font-size: clamp(4.2rem, 9vw, 8.1rem);
		font-weight: 800;
		line-height: 0.78;
		letter-spacing: -0.045em;
		text-transform: uppercase;
	}

	.battle-word {
		color: #fff;
		text-shadow: 0 0 44px rgba(255, 255, 255, 0.08);
	}

	.report-word {
		color: transparent;
		-webkit-text-stroke: 1.5px rgba(200, 255, 55, 0.92);
		text-shadow: 0 0 34px rgba(200, 255, 55, 0.12);
	}

	.hero-lede {
		max-width: 48rem;
		margin: 0;
		font-size: clamp(1rem, 1.5vw, 1.15rem);
		line-height: 1.8;
		color: rgba(226, 232, 240, 0.76);
	}

	.principles {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0.6rem;
		margin-top: 2rem;
	}

	.principle {
		display: flex;
		gap: 0.8rem;
		min-width: 0;
		padding: 0.85rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 12px;
		background: rgba(255, 255, 255, 0.025);
	}

	.principle-number {
		font-family: 'JetBrains Mono Variable', monospace;
		font-size: 0.72rem;
		color: var(--acid);
	}

	.principle div {
		display: grid;
		gap: 0.1rem;
		min-width: 0;
	}

	.principle strong {
		font-size: 0.84rem;
		color: rgba(255, 255, 255, 0.9);
	}

	.principle div span {
		font-size: 0.72rem;
		color: rgba(255, 255, 255, 0.42);
	}

	.integrity-note {
		display: flex;
		gap: 0.8rem;
		align-items: flex-start;
		margin-top: 1rem;
		padding: 0.85rem 1rem;
		border-left: 2px solid rgba(200, 255, 55, 0.7);
		background: linear-gradient(90deg, rgba(200, 255, 55, 0.07), transparent);
		font-size: 0.8rem;
		line-height: 1.6;
		color: rgba(255, 255, 255, 0.54);
	}

	.integrity-note svg {
		width: 1.05rem;
		flex: 0 0 auto;
		fill: var(--acid);
	}

	.integrity-note strong {
		color: rgba(255, 255, 255, 0.86);
	}

	.visual-shell {
		position: relative;
		display: flex;
		min-height: 30rem;
		flex-direction: column;
		overflow: hidden;
		border: 1px solid rgba(200, 255, 55, 0.17);
		border-radius: 18px;
		background:
			linear-gradient(145deg, rgba(200, 255, 55, 0.045), transparent 35%),
			rgba(3, 8, 9, 0.82);
		box-shadow:
			inset 0 0 80px rgba(200, 255, 55, 0.025),
			0 24px 60px rgba(0, 0, 0, 0.28);
	}

	.visual-shell::after {
		position: absolute;
		inset: 0;
		content: '';
		pointer-events: none;
		background: linear-gradient(
			115deg,
			transparent 0 40%,
			rgba(255, 255, 255, 0.055) 48%,
			transparent 56%
		);
		transform: translateX(-100%);
		animation: glass-scan 7s ease-in-out infinite;
	}

	.hud-topline,
	.telemetry,
	.data-stream {
		font-family: 'JetBrains Mono Variable', 'JetBrains Mono', monospace;
	}

	.hud-topline {
		display: flex;
		justify-content: space-between;
		padding: 1rem 1.1rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.07);
		font-size: 0.62rem;
		letter-spacing: 0.14em;
		color: rgba(255, 255, 255, 0.45);
	}

	.hud-online {
		color: var(--acid);
	}

	.radar {
		position: relative;
		width: min(78%, 24rem);
		aspect-ratio: 1;
		margin: auto;
		overflow: hidden;
		border: 1px solid rgba(200, 255, 55, 0.24);
		border-radius: 50%;
		background:
			repeating-radial-gradient(
				circle at center,
				transparent 0 18%,
				rgba(200, 255, 55, 0.075) 18.3% 18.7%,
				transparent 19% 36%
			),
			linear-gradient(90deg, transparent 49.8%, rgba(200, 255, 55, 0.09) 50%, transparent 50.2%),
			linear-gradient(0deg, transparent 49.8%, rgba(200, 255, 55, 0.09) 50%, transparent 50.2%),
			radial-gradient(circle at center, rgba(200, 255, 55, 0.075), transparent 62%);
		box-shadow:
			inset 0 0 55px rgba(200, 255, 55, 0.075),
			0 0 50px rgba(200, 255, 55, 0.05);
	}

	.radar::before {
		position: absolute;
		inset: 0;
		content: '';
		border-radius: inherit;
		background-image:
			linear-gradient(rgba(200, 255, 55, 0.04) 1px, transparent 1px),
			linear-gradient(90deg, rgba(200, 255, 55, 0.04) 1px, transparent 1px);
		background-size: 20px 20px;
		mask-image: radial-gradient(circle, black, transparent 72%);
	}

	.sweep {
		position: absolute;
		inset: -1px;
		border-radius: 50%;
		background: conic-gradient(
			from 0deg,
			rgba(200, 255, 55, 0) 0deg,
			rgba(200, 255, 55, 0) 310deg,
			rgba(200, 255, 55, 0.25) 346deg,
			rgba(200, 255, 55, 0.03) 360deg
		);
		animation: sweep 4.8s linear infinite;
	}

	.radar-ring {
		position: absolute;
		inset: 50%;
		border: 1px solid rgba(200, 255, 55, 0.16);
		border-radius: 50%;
		transform: translate(-50%, -50%);
	}

	.ring-a {
		width: 38%;
		height: 38%;
	}

	.ring-b {
		width: 66%;
		height: 66%;
	}

	.ring-c {
		width: 90%;
		height: 90%;
	}

	.reticle {
		position: absolute;
		inset: 25%;
		width: 50%;
		height: 50%;
		fill: none;
		stroke: rgba(255, 255, 255, 0.35);
		stroke-width: 1.2;
	}

	.reticle-spin {
		transform-origin: center;
		animation: reverse-spin 12s linear infinite;
	}

	.reticle-core {
		fill: var(--acid);
		stroke: none;
		filter: drop-shadow(0 0 6px var(--acid));
	}

	.blip {
		position: absolute;
		width: 0.52rem;
		height: 0.52rem;
		border-radius: 50%;
		background: var(--acid);
		box-shadow: 0 0 14px rgba(200, 255, 55, 0.8);
	}

	.blip span {
		position: absolute;
		inset: -0.45rem;
		border: 1px solid rgba(200, 255, 55, 0.5);
		border-radius: 50%;
		animation: blip 1.8s ease-out infinite;
	}

	.blip-a {
		top: 31%;
		left: 63%;
	}

	.blip-b {
		top: 66%;
		left: 34%;
		animation-delay: -0.8s;
	}

	.blip-b span {
		animation-delay: -0.8s;
	}

	.blip-c {
		top: 49%;
		left: 78%;
		background: var(--red);
		box-shadow: 0 0 14px rgba(255, 93, 98, 0.75);
	}

	.blip-c span {
		border-color: rgba(255, 93, 98, 0.5);
		animation-delay: -1.25s;
	}

	.telemetry {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		border-top: 1px solid rgba(255, 255, 255, 0.07);
	}

	.telemetry div {
		display: grid;
		gap: 0.35rem;
		padding: 0.85rem 1rem;
		border-right: 1px solid rgba(255, 255, 255, 0.07);
	}

	.telemetry div:last-child {
		border-right: 0;
	}

	.telemetry span {
		font-size: 0.55rem;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: rgba(255, 255, 255, 0.36);
	}

	.telemetry strong {
		font-size: 0.75rem;
		color: rgba(255, 255, 255, 0.82);
	}

	.data-stream {
		position: absolute;
		right: 1rem;
		bottom: 5.3rem;
		left: 1rem;
		display: grid;
		gap: 0.2rem;
		overflow: hidden;
		font-size: 0.47rem;
		letter-spacing: 0.11em;
		color: rgba(200, 255, 55, 0.2);
		pointer-events: none;
	}

	.data-stream span {
		white-space: nowrap;
		animation: stream 10s linear infinite;
	}

	.data-stream span:last-child {
		animation-direction: reverse;
		animation-duration: 13s;
	}

	.report-layout {
		display: grid;
		grid-template-columns: minmax(250px, 0.68fr) minmax(0, 1.32fr);
		gap: 1rem;
		padding: 0 clamp(1rem, 2.4vw, 2rem) clamp(1rem, 2.4vw, 2rem);
	}

	.case-brief,
	.form-card {
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 16px;
		background: rgba(255, 255, 255, 0.025);
		backdrop-filter: blur(14px);
	}

	.case-brief {
		padding: clamp(1.4rem, 2.5vw, 2rem);
	}

	.case-brief h2,
	.form-card h2,
	.success-state h3,
	.auth-state h3 {
		margin: 0.45rem 0 0;
		font-family: 'Barlow Condensed', 'Arial Narrow', sans-serif;
		font-weight: 700;
		letter-spacing: 0.01em;
		color: #fff;
	}

	.case-brief h2,
	.form-card h2 {
		font-size: clamp(1.7rem, 2.8vw, 2.35rem);
	}

	.case-brief > p:not(.section-kicker) {
		margin: 1rem 0 0;
		font-size: 0.86rem;
		line-height: 1.75;
		color: rgba(255, 255, 255, 0.5);
	}

	.brief-list {
		display: grid;
		gap: 0.7rem;
		margin-top: 1.4rem;
	}

	.brief-list > div {
		display: grid;
		grid-template-columns: 1.8rem 1fr;
		gap: 0.7rem;
		padding-top: 0.8rem;
		border-top: 1px solid rgba(255, 255, 255, 0.07);
	}

	.brief-index {
		font-family: 'JetBrains Mono Variable', monospace;
		font-size: 0.72rem;
		color: var(--acid);
	}

	.brief-list p {
		margin: 0;
		font-size: 0.76rem;
		line-height: 1.55;
		color: rgba(255, 255, 255, 0.45);
	}

	.brief-list strong {
		font-size: 0.81rem;
		color: rgba(255, 255, 255, 0.84);
	}

	.form-card {
		padding: clamp(1.4rem, 3vw, 2.4rem);
		background:
			linear-gradient(145deg, rgba(255, 255, 255, 0.045), transparent 40%),
			rgba(10, 14, 16, 0.75);
	}

	.form-card-head {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 1rem;
		margin-bottom: 1.8rem;
	}

	.case-chip {
		padding: 0.45rem 0.58rem;
		border: 1px solid rgba(200, 255, 55, 0.2);
		border-radius: 7px;
		background: rgba(200, 255, 55, 0.05);
		font-family: 'JetBrains Mono Variable', monospace;
		font-size: 0.58rem;
		letter-spacing: 0.13em;
		color: rgba(200, 255, 55, 0.72);
	}

	form {
		display: grid;
		gap: 1.3rem;
	}

	.field-block {
		min-width: 0;
		margin: 0;
		padding: 0;
		border: 0;
	}

	.field-heading {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		margin-bottom: 0.55rem;
	}

	.field-heading label,
	.field-heading legend {
		font-size: 0.82rem;
		font-weight: 700;
		color: rgba(255, 255, 255, 0.86);
	}

	.field-heading span {
		font-family: 'JetBrains Mono Variable', monospace;
		font-size: 0.57rem;
		letter-spacing: 0.11em;
		color: rgba(255, 255, 255, 0.28);
	}

	.input-wrap {
		position: relative;
	}

	.input-wrap svg {
		position: absolute;
		top: 50%;
		left: 0.9rem;
		width: 1rem;
		transform: translateY(-50%);
		fill: none;
		stroke: rgba(255, 255, 255, 0.35);
		stroke-width: 1.6;
	}

	input,
	textarea {
		width: 100%;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 10px;
		outline: 0;
		background: rgba(0, 0, 0, 0.26);
		color: #fff;
		transition:
			border-color 160ms ease,
			box-shadow 160ms ease,
			background 160ms ease;
	}

	input {
		min-height: 3.25rem;
		padding: 0.8rem 0.9rem 0.8rem 2.7rem;
	}

	textarea {
		resize: vertical;
		min-height: 8.8rem;
		padding: 0.9rem;
		line-height: 1.65;
	}

	input::placeholder,
	textarea::placeholder {
		color: rgba(255, 255, 255, 0.25);
	}

	input:focus,
	textarea:focus {
		border-color: rgba(200, 255, 55, 0.45);
		background: rgba(200, 255, 55, 0.025);
		box-shadow: 0 0 0 3px rgba(200, 255, 55, 0.055);
	}

	.field-help {
		margin: 0.45rem 0 0;
		font-size: 0.67rem;
		line-height: 1.5;
		color: rgba(255, 255, 255, 0.35);
	}

	.category-grid {
		display: grid;
		grid-template-columns: repeat(5, minmax(0, 1fr));
		gap: 0.45rem;
	}

	.category-grid button {
		display: grid;
		gap: 0.25rem;
		min-height: 5.1rem;
		padding: 0.75rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 10px;
		text-align: left;
		background: rgba(255, 255, 255, 0.02);
		cursor: pointer;
		transition:
			transform 160ms ease,
			border-color 160ms ease,
			background 160ms ease;
	}

	.category-grid button:hover {
		transform: translateY(-2px);
		border-color: rgba(255, 255, 255, 0.17);
		background: rgba(255, 255, 255, 0.04);
	}

	.category-grid strong {
		font-size: 0.75rem;
		color: rgba(255, 255, 255, 0.79);
	}

	.category-grid span {
		font-size: 0.59rem;
		line-height: 1.35;
		color: rgba(255, 255, 255, 0.33);
	}

	.category-grid .category-active {
		border-color: rgba(200, 255, 55, 0.48);
		background: linear-gradient(145deg, rgba(200, 255, 55, 0.11), rgba(200, 255, 55, 0.025));
		box-shadow: inset 0 0 22px rgba(200, 255, 55, 0.025);
	}

	.category-grid .category-active strong {
		color: var(--acid);
	}

	.submit-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		padding-top: 0.2rem;
	}

	.storage-note {
		display: flex;
		max-width: 28rem;
		gap: 0.55rem;
		align-items: center;
		font-size: 0.67rem;
		line-height: 1.45;
		color: rgba(255, 255, 255, 0.36);
	}

	.primary-action,
	.secondary-action {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 0.9rem;
		min-height: 2.85rem;
		border-radius: 9px;
		font-weight: 800;
		text-decoration: none;
		cursor: pointer;
		transition:
			transform 160ms ease,
			box-shadow 160ms ease,
			opacity 160ms ease;
	}

	.primary-action {
		padding: 0.75rem 1rem;
		border: 1px solid rgba(200, 255, 55, 0.74);
		background: var(--acid);
		color: #0a0d0d;
		box-shadow: 0 8px 30px rgba(200, 255, 55, 0.1);
	}

	.primary-action:hover:not(:disabled) {
		transform: translateY(-2px);
		box-shadow: 0 12px 34px rgba(200, 255, 55, 0.18);
	}

	.primary-action:disabled {
		cursor: wait;
		opacity: 0.58;
	}

	.submit-action {
		flex: 0 0 auto;
		min-width: 9.6rem;
	}

	.secondary-action {
		margin-top: 1.2rem;
		padding: 0.72rem 0.95rem;
		border: 1px solid rgba(255, 255, 255, 0.12);
		background: rgba(255, 255, 255, 0.04);
		color: rgba(255, 255, 255, 0.78);
	}

	.success-state,
	.auth-state {
		display: flex;
		min-height: 24rem;
		flex-direction: column;
		align-items: flex-start;
		justify-content: center;
	}

	.success-state h3,
	.auth-state h3 {
		font-size: 2rem;
	}

	.success-state > p:not(.section-kicker),
	.auth-state > p {
		max-width: 38rem;
		margin: 0.85rem 0 0;
		font-size: 0.84rem;
		line-height: 1.75;
		color: rgba(255, 255, 255, 0.5);
	}

	.success-mark,
	.auth-icon {
		display: grid;
		width: 3rem;
		height: 3rem;
		margin-bottom: 1rem;
		place-items: center;
		border-radius: 12px;
	}

	.success-mark {
		border: 1px solid rgba(200, 255, 55, 0.28);
		background: rgba(200, 255, 55, 0.08);
	}

	.auth-icon {
		border: 1px solid rgba(103, 232, 249, 0.2);
		background: rgba(103, 232, 249, 0.06);
	}

	.success-mark svg,
	.auth-icon svg {
		width: 1.55rem;
		fill: none;
		stroke: var(--acid);
		stroke-width: 2;
	}

	.auth-icon svg {
		stroke: var(--cyan);
	}

	.receipt {
		display: grid;
		width: 100%;
		grid-template-columns: 0.7fr 1.3fr;
		margin-top: 1.2rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 10px;
		background: rgba(0, 0, 0, 0.18);
	}

	.receipt div {
		display: grid;
		gap: 0.28rem;
		padding: 0.85rem 1rem;
	}

	.receipt div + div {
		border-left: 1px solid rgba(255, 255, 255, 0.08);
	}

	.receipt span {
		font-size: 0.6rem;
		color: rgba(255, 255, 255, 0.34);
	}

	.receipt strong {
		overflow: hidden;
		font-family: 'JetBrains Mono Variable', monospace;
		font-size: 0.78rem;
		text-overflow: ellipsis;
		color: rgba(255, 255, 255, 0.8);
	}

	.auth-state .primary-action {
		margin-top: 1.2rem;
	}

	.account-link {
		margin-top: 0.85rem;
		font-size: 0.7rem;
		color: rgba(255, 255, 255, 0.42);
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.error-state {
		display: grid;
		gap: 0.2rem;
		margin-top: 1rem;
		padding: 0.8rem 0.9rem;
		border: 1px solid rgba(255, 93, 98, 0.2);
		border-radius: 9px;
		background: rgba(255, 93, 98, 0.055);
		font-size: 0.73rem;
		color: rgba(255, 255, 255, 0.58);
	}

	.error-state strong {
		color: #ff8f93;
	}

	@keyframes sweep {
		to {
			transform: rotate(360deg);
		}
	}

	@keyframes reverse-spin {
		to {
			transform: rotate(-360deg);
		}
	}

	@keyframes blip {
		0% {
			transform: scale(0.25);
			opacity: 0.9;
		}
		100% {
			transform: scale(1.8);
			opacity: 0;
		}
	}

	@keyframes pulse {
		0% {
			box-shadow: 0 0 0 0 rgba(200, 255, 55, 0.38);
		}
		70% {
			box-shadow: 0 0 0 8px rgba(200, 255, 55, 0);
		}
		100% {
			box-shadow: 0 0 0 0 rgba(200, 255, 55, 0);
		}
	}

	@keyframes drift {
		to {
			transform: translate3d(3rem, 1rem, 0) scale(1.08);
		}
	}

	@keyframes glass-scan {
		0%,
		18% {
			transform: translateX(-120%);
		}
		62%,
		100% {
			transform: translateX(120%);
		}
	}

	@keyframes stream {
		from {
			transform: translateX(0);
		}
		to {
			transform: translateX(-18%);
		}
	}

	@media (max-width: 1050px) {
		.hero-grid {
			grid-template-columns: 1fr;
		}

		.visual-shell {
			min-height: 27rem;
		}

		.radar {
			width: min(58%, 23rem);
		}

		.category-grid {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
	}

	@media (max-width: 760px) {
		.battle-report {
			border-radius: 16px;
		}

		.hero-grid {
			padding: 1.35rem;
		}

		h1 {
			font-size: clamp(3.7rem, 21vw, 6rem);
		}

		.principles {
			grid-template-columns: 1fr;
		}

		.visual-shell {
			min-height: 24rem;
		}

		.radar {
			width: min(72%, 20rem);
		}

		.report-layout {
			grid-template-columns: 1fr;
			padding: 0 0.75rem 0.75rem;
		}

		.category-grid {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}

		.submit-row {
			align-items: stretch;
			flex-direction: column;
		}

		.submit-action {
			width: 100%;
		}

		.telemetry div {
			padding: 0.72rem;
		}
	}

	@media (max-width: 460px) {
		.category-grid {
			grid-template-columns: 1fr;
		}

		.receipt {
			grid-template-columns: 1fr;
		}

		.receipt div + div {
			border-top: 1px solid rgba(255, 255, 255, 0.08);
			border-left: 0;
		}

		.telemetry {
			grid-template-columns: 1fr;
		}

		.telemetry div {
			grid-template-columns: 1fr 1fr;
			border-right: 0;
			border-bottom: 1px solid rgba(255, 255, 255, 0.07);
		}

		.telemetry div:last-child {
			border-bottom: 0;
		}

		.data-stream {
			bottom: 8.8rem;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		*,
		*::before,
		*::after {
			scroll-behavior: auto !important;
			animation-duration: 0.001ms !important;
			animation-iteration-count: 1 !important;
		}
	}
</style>
