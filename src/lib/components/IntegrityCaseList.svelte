<script lang="ts">
	import AiCaseSummary from '$lib/components/AiCaseSummary.svelte';
	import { api, errorMessage } from '$lib/api';
	import { invalidateAll } from '$app/navigation';
	import { integrityCaseStatus, integrityPartText } from '$lib/integrity-display';
	import type { StatisticalAssessment } from '$lib/server/integrity/statistics';
	import type { PageData } from '../../routes/(app)/server/[id]/integrity/$types';
	type CaseData = Pick<
		PageData,
		'cases' | 'labels' | 'aiJobs' | 'aiAutoEnabled' | 'reviewPenalties' | 'canConfigure'
	> & { server: { id: string; orgId: string } };
	let {
		data,
		lang = 'zh',
		archiveUrl
	}: { data: CaseData; lang?: 'zh' | 'en'; archiveUrl?: string } = $props();
	let t = $derived(
		lang === 'zh'
			? {
					cases: '证据案件',
					time: '时间',
					player: '玩家',
					caseId: '案件编号',
					risk: '风险分',
					confidence: '置信度',
					status: '状态',
					breakdown: '证据与审核',
					noCases: '暂无证据案件。'
				}
			: {
					cases: 'Evidence cases',
					time: 'Time',
					player: 'Player',
					caseId: 'Case ID',
					risk: 'Risk',
					confidence: 'Confidence',
					status: 'Status',
					breakdown: 'Evidence and review',
					noCases: 'No evidence cases.'
				}
	);
	let pendingLabel = $state<Record<string, string>>({});
	let pendingReason = $state<Record<string, string>>({});
	let labelError = $state('');
	let labelNotice = $state('');
	let editingReview = $state<Record<string, boolean>>({});
	const reviewNames: Record<string, string> = {
		FALSE_POSITIVE: '误判',
		CONFIRMED_ABUSE: '确认违规',
		INSUFFICIENT_EVIDENCE: '证据不足',
		DATA_ERROR: '数据错误'
	};
	let savingLabel = $state<string | null>(null);
	async function saveLabel(caseId: string) {
		if (savingLabel !== null) return;
		labelError = '';
		labelNotice = '';
		savingLabel = caseId;
		try {
			const result = await api<{
				label: { penalty: { reused: boolean; expiresAt: string | null } | null };
			}>(
				'POST',
				`/api/orgs/${encodeURIComponent(data.server.orgId)}/integrity/cases/${encodeURIComponent(caseId)}/labels`,
				{ label: pendingLabel[caseId], reason: pendingReason[caseId] }
			);
			labelNotice =
				`案件 ${caseId} 审核已保存。` +
				(result.label.penalty
					? result.label.penalty.reused
						? '沿用该案件已有处罚，未重复封禁或延长时间。'
						: '封禁已保存，即时踢出已排队；具体状态见案件处罚记录。'
					: '');
			editingReview[caseId] = false;
			pendingReason[caseId] = '';
			pendingLabel[caseId] = '';
			try {
				await invalidateAll();
			} catch {
				labelError = '审核已保存，但页面刷新失败，请重新加载页面。';
			}
		} catch (err) {
			labelError = errorMessage(err);
		} finally {
			savingLabel = null;
		}
	}
	const when = (value: string) => new Date(value).toLocaleString(lang === 'zh' ? 'zh-CN' : 'en-US');
	const parts = (value: unknown): { code: string; points: number; detail: string }[] =>
		Array.isArray(value)
			? value.filter(
					(item): item is { code: string; points: number; detail: string } =>
						!!item && typeof item.code === 'string' && typeof item.points === 'number'
				)
			: [];
	const caseSignals = (snapshot: unknown): string => {
		if (!snapshot || typeof snapshot !== 'object') return '';
		const data = snapshot as Record<string, unknown>;
		const names: Record<string, [string, string]> = {
			kpm: ['步兵 KPM', 'Infantry KPM'],
			headshot: ['爆头率', 'Headshot rate'],
			penetration: ['穿透率', 'Penetration rate'],
			burst: ['短时爆发', 'Kill burst']
		};
		return Array.isArray(data.behaviorReasons)
			? data.behaviorReasons
					.filter((reason): reason is string => typeof reason === 'string')
					.map((reason) => names[reason]?.[lang === 'zh' ? 0 : 1] ?? reason)
					.join(' · ')
			: '';
	};
	const deliveryLabel = (state: string | null) =>
		(lang === 'zh'
			? {
					pending: '待发送',
					delivered: '已送达',
					failed: '发送失败',
					skipped: '已跳过',
					unknown: '结果未知'
				}
			: {
					pending: 'Pending',
					delivered: 'Delivered',
					failed: 'Failed',
					skipped: 'Skipped',
					unknown: 'Unknown'
				})[(state ?? 'unknown') as 'pending' | 'delivered' | 'failed' | 'skipped' | 'unknown'];
</script>

<section class="mb-6 panel p-4">
	<h3 class="mb-3 text-base font-semibold text-white">{t.cases}</h3>
	{#if archiveUrl}<p class="mb-3 text-sm text-mist-300">
			仅展示最近5条，更多案件自动收进历史列表。<a class="ml-3 btn" href={archiveUrl}
				>查看全部／归档 →</a
			>
		</p>{/if}
	{#if labelNotice}<p class="mb-2 text-sm text-accent" role="status">{labelNotice}</p>{/if}
	{#if labelError}<p class="mb-2 text-sm text-warn">{labelError}</p>{/if}
	{#if data.cases.length}
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>{t.time}</th><th>{t.player}</th><th>{t.caseId}</th><th>{t.risk}</th><th
							>{t.confidence}</th
						><th>{t.status}</th><th>{t.breakdown}</th></tr
					></thead
				>
				<tbody>
					{#each data.cases as item (item.id)}
						{@const latest = data.labels.find((row) => row.caseId === item.id)}
						<tr>
							<td class="whitespace-nowrap">{when(item.createdAt)}</td>
							<td
								><a
									class="font-mono text-accent"
									href="/server/{data.server.id}/players/{item.steamId}"
									>{item.name || item.steamId} {item.name ? `· ${item.steamId.slice(-6)}` : ''}</a
								></td
							>
							<td class="font-mono">{item.id}</td>
							<td>{item.riskScore}</td>
							<td>{item.confidence}</td>
							<td
								>{latest
									? '已审核'
									: lang === 'zh'
										? integrityCaseStatus(item.status)
										: item.status}
								{#if latest}<div class="mt-1 text-xs text-mist-300">
										{reviewNames[latest.label] ?? latest.label} · {when(latest.createdAt)}
									</div>
									<div class="mt-1 text-xs text-mist-400">{latest.reason}</div>{/if}
							</td>
							<td>
								<AiCaseSummary
									job={data.aiJobs.find((j) => j.caseId === item.id)}
									enabled={data.aiAutoEnabled}
								/>
								<details>
									<summary class="cursor-pointer">{t.breakdown}</summary>
									{#if caseSignals(item.snapshot)}<p class="mt-2 text-xs text-accent">
											{caseSignals(item.snapshot)}
										</p>{/if}
									<ul class="mt-2 space-y-1 text-xs">
										{#each parts(item.riskBreakdown) as part (part.code)}
											<li>+{part.points} {integrityPartText(part.code, part.detail, lang)}</li>
										{/each}
									</ul>
									{#if item.statistical}<p class="mt-2 text-xs text-mist-300">
											委员会：{(item.statistical as StatisticalAssessment).committee?.decision ??
												'UNKNOWN'}
										</p>{/if}
									{#if latest}<p class="mt-2 text-xs text-mist-300">
											审核结论：{reviewNames[latest.label] ?? latest.label} · {latest.reason}
										</p>{/if}

									{#each data.reviewPenalties.filter((p) => p.caseId === item.id) as penalty (penalty.id)}
										<p class="mt-2 text-xs text-amber-300">
											人工审核处罚：{penalty.active ? '封禁有效' : '封禁已到期或已撤销'} · 到期：{penalty.expiresAt
												? when(penalty.expiresAt)
												: '永久（沿用已有封禁）'} · 即时踢出：{deliveryLabel(penalty.deliveryState)}
										</p>
									{/each}
									{#if data.canConfigure && latest && !editingReview[item.id]}<button
											class="btn-secondary mt-2 btn text-xs"
											onclick={() => {
												editingReview[item.id] = true;
												pendingLabel[item.id] = latest.label;
												pendingReason[item.id] = latest.reason;
											}}>修改审核结论</button
										>{/if}
									{#if data.canConfigure && (!latest || editingReview[item.id])}<div
											class="mt-3 flex flex-wrap items-center gap-2"
										>
											<p class="w-full text-xs text-mist-300">
												确认违规会在本服务器立即封禁7天并排队踢出；已有更长期或永久封禁予以保留。重复保存不会续期。修改审核结论不会撤销已有封禁，撤销请到封禁管理操作。
											</p>
											<select class="input text-xs" bind:value={pendingLabel[item.id]}
												><option value="">选择审核结论</option><option value="FALSE_POSITIVE"
													>误判</option
												><option value="CONFIRMED_ABUSE">确认违规／作弊（封禁7天）</option><option
													value="INSUFFICIENT_EVIDENCE">证据不足</option
												><option value="DATA_ERROR">数据错误</option></select
											><input
												class="input text-xs"
												placeholder="填写审核理由（至少 5 字）"
												bind:value={pendingReason[item.id]}
											/><button
												class="btn-secondary btn text-xs"
												disabled={savingLabel !== null ||
													!pendingLabel[item.id] ||
													(pendingReason[item.id]?.trim().length ?? 0) < 5}
												onclick={() => saveLabel(item.id)}
												>{savingLabel === item.id
													? '正在保存…'
													: pendingLabel[item.id] === 'CONFIRMED_ABUSE'
														? '确认违规并封禁7天'
														: '保存审核结论'}</button
											>
										</div>{/if}
								</details>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}<p class="text-sm text-mist-400">{t.noCases}</p>{/if}
</section>
