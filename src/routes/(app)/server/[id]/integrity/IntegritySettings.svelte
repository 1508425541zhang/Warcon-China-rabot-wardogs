<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import type { IntegrityRuleConfig } from '$lib/server/integrity/score';
	import type { WeaponCategory } from '$lib/server/integrity/weapons';

	type Rules = IntegrityRuleConfig;
	type NumericKey = {
		[K in keyof Rules]: Rules[K] extends number ? K : never;
	}[keyof Rules];
	type Field = { key: NumericKey; zh: string; en: string; min: number; max: number; step?: number };
	let {
		orgId,
		config,
		ruleDefaults,
		overrides,
		weaponDefaults,
		categories,
		lang
	}: {
		orgId: string;
		config: Rules;
		ruleDefaults: Rules;
		overrides: { cause: string; category: string }[];
		weaponDefaults: Readonly<Record<string, WeaponCategory>>;
		categories: readonly WeaponCategory[];
		lang: 'zh' | 'en';
	} = $props();
	// svelte-ignore state_referenced_locally -- initialise the editable snapshot for SSR; the effect follows later prop updates.
	let draft = $state<Rules>(structuredClone(config));
	$effect(() => {
		draft = structuredClone(config);
	});
	let cause = $state('');
	let category = $state('INFANTRY');
	let busy = $state(false);
	let notice = $state('');
	let problem = $state('');
	let ranges = $derived([
		{ start: 0, end: draft.passiveWatchThreshold - 1, zh: '正常', en: 'Normal' },
		{
			start: draft.passiveWatchThreshold,
			end: draft.activeWatchThreshold - 1,
			zh: '被动观察',
			en: 'Passive watch'
		},
		{
			start: draft.activeWatchThreshold,
			end: draft.koThreshold - 1,
			zh: '主动观察',
			en: 'Active watch'
		},
		{
			start: draft.koThreshold,
			end: draft.quarantineThreshold - 1,
			zh: '达到移出阈值',
			en: 'KO threshold met'
		},
		{
			start: draft.quarantineThreshold,
			end: 100,
			zh: '达到隔离资格阈值',
			en: 'Quarantine eligibility'
		}
	]);
	let validationError = $derived.by(() => {
		if (!(
			draft.passiveWatchThreshold < draft.activeWatchThreshold &&
			draft.activeWatchThreshold < draft.koThreshold &&
			draft.koThreshold < draft.quarantineThreshold
		))
			return lang === 'zh'
				? '风险阈值必须依次递增：被动观察 < 主动观察 < 移出 < 隔离。'
				: 'Risk thresholds must increase from passive watch through quarantine.';
		for (const [name, bands] of [
			['KPM', draft.kpmBands],
			[lang === 'zh' ? '独立受害者' : 'Unique victims', draft.uniqueVictimBands],
			[lang === 'zh' ? '独立举报人' : 'Unique reporters', draft.reportBands]
		] as const) {
			if (
				bands.some(
					(band, i) => i > 0 && (band.min <= bands[i - 1].min || band.points < bands[i - 1].points)
				)
			)
				return lang === 'zh'
					? `${name} 的门槛必须递增，加分不能递减。`
					: `${name} thresholds must increase and points cannot decrease.`;
		}
		if (draft.oldVac > draft.recentVac || draft.oldGameBan > draft.recentGameBan)
			return lang === 'zh'
				? '旧封禁加分不能高于近期封禁加分。'
				: 'Older bans cannot add more points than recent bans.';
		return '';
	});

	const fields: { headingZh: string; headingEn: string; entries: Field[] }[] = [
		{
			headingZh: '风险等级与处罚预览',
			headingEn: 'Risk levels and action preview',
			entries: [
				{
					key: 'passiveWatchThreshold',
					zh: '被动观察起点',
					en: 'Passive watch threshold',
					min: 1,
					max: 99
				},
				{
					key: 'activeWatchThreshold',
					zh: '主动观察起点',
					en: 'Active watch threshold',
					min: 1,
					max: 99
				},
				{ key: 'koThreshold', zh: '自动移出阈值', en: 'Auto KO threshold', min: 1, max: 100 },
				{
					key: 'quarantineThreshold',
					zh: '隔离资格阈值',
					en: 'Quarantine threshold',
					min: 1,
					max: 100
				}
			]
		},
		{
			headingZh: '独立窗口与辅助信号',
			headingEn: 'Independent windows and auxiliary signals',
			entries: [
				{
					key: 'repeatWindowMinutes',
					zh: '重复窗口观察期（分钟）',
					en: 'Repeat window period (min)',
					min: 1,
					max: 120
				},
				{
					key: 'repeatSecond',
					zh: '第二个异常窗口加分',
					en: 'Second abnormal window points',
					min: 0,
					max: 30
				},
				{
					key: 'repeatThird',
					zh: '第三个异常窗口加分',
					en: 'Third abnormal window points',
					min: 0,
					max: 30
				},
				{
					key: 'repeatBoth5',
					zh: '两个窗口均 ≥5 KPM 加分',
					en: 'Two windows ≥5 KPM points',
					min: 0,
					max: 30
				},
				{
					key: 'repeatBoth6',
					zh: '两个窗口均 ≥6 KPM 加分',
					en: 'Two windows ≥6 KPM points',
					min: 0,
					max: 30
				},
				{
					key: 'repeatKo',
					zh: '再次达到自动移出阈值加分',
					en: 'Repeat Auto KO points',
					min: 0,
					max: 30
				},
				{
					key: 'repeatKoWindowHours',
					zh: '重复高风险窗口回顾期（小时）',
					en: 'Repeat high-risk window review period (hours)',
					min: 1,
					max: 720
				},
				{
					key: 'headshotMinKills',
					zh: '爆头率最低样本击杀',
					en: 'Headshot sample kills',
					min: 1,
					max: 100
				},
				{
					key: 'headshotMinPct',
					zh: '爆头率门槛（%）',
					en: 'Headshot threshold (%)',
					min: 1,
					max: 100
				},
				{
					key: 'headshotMax',
					zh: '爆头率最高加分',
					en: 'Headshot maximum points',
					min: 0,
					max: 10
				},
				{
					key: 'penetrationMinKills',
					zh: '穿透率最低样本击杀',
					en: 'Penetration sample kills',
					min: 1,
					max: 100
				},
				{
					key: 'penetrationMinPct',
					zh: '穿透率门槛（%）',
					en: 'Penetration threshold (%)',
					min: 1,
					max: 100
				},
				{
					key: 'penetrationMax',
					zh: '穿透率最高加分',
					en: 'Penetration maximum points',
					min: 0,
					max: 6
				},
				{
					key: 'burstMax',
					zh: '短时击杀最高加分',
					en: 'Kill burst maximum points',
					min: 1,
					max: 20
				},
				{
					key: 'burstFindingMin',
					zh: '短时爆发独立触发点数',
					en: 'Burst finding threshold',
					min: 1,
					max: 12
				}
			]
		},
		{
			headingZh: 'Steam 与游戏时间先验',
			headingEn: 'Steam and playtime priors',
			entries: [
				{ key: 'steamPriorCap', zh: 'Steam 处罚先验上限', en: 'Steam prior cap', min: 0, max: 30 },
				{ key: 'recentVac', zh: '近期 VAC 加分', en: 'Recent VAC points', min: 0, max: 30 },
				{
					key: 'recentGameBan',
					zh: '近期游戏封禁加分',
					en: 'Recent game ban points',
					min: 0,
					max: 30
				},
				{ key: 'oldVac', zh: '旧 VAC 加分', en: 'Old VAC points', min: 0, max: 30 },
				{ key: 'oldGameBan', zh: '旧游戏封禁加分', en: 'Old game ban points', min: 0, max: 30 },
				{
					key: 'lowPlaytimeHours',
					zh: '低游戏时间门槛（小时）',
					en: 'Low playtime hours',
					min: 0,
					max: 100,
					step: 0.1
				},
				{
					key: 'lowPlaytimeKpm',
					zh: '低游戏时间需达到 KPM',
					en: 'Low playtime KPM',
					min: 1,
					max: 20,
					step: 0.1
				},
				{
					key: 'lowPlaytimePoints',
					zh: '低游戏时间加分',
					en: 'Low playtime points',
					min: 0,
					max: 15
				}
			]
		}
	];

	async function saveRules() {
		if (validationError) {
			problem = validationError;
			return;
		}
		busy = true;
		problem = notice = '';
		try {
			const { mode: _legacyMode, quarantineDays: _legacyDays, ...values } = draft;
			await api('PUT', `/api/orgs/${encodeURIComponent(orgId)}/integrity/rules`, { values });
			notice = lang === 'zh' ? '规则已保存并生成新版本。' : 'Rules saved as a new version.';
			await invalidateAll();
		} catch (err) {
			problem = errorMessage(err);
		} finally {
			busy = false;
		}
	}
	async function saveWeapon() {
		busy = true;
		problem = notice = '';
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(orgId)}/integrity/weapons`, {
				cause: cause.trim(),
				category
			});
			cause = '';
			notice = lang === 'zh' ? '武器分类已保存。' : 'Weapon mapping saved.';
			await invalidateAll();
		} catch (err) {
			problem = errorMessage(err);
		} finally {
			busy = false;
		}
	}
	async function removeWeapon(value: string) {
		busy = true;
		problem = notice = '';
		try {
			await api('DELETE', `/api/orgs/${encodeURIComponent(orgId)}/integrity/weapons`, {
				cause: value
			});
			notice = lang === 'zh' ? '自定义分类已移除。' : 'Override removed.';
			await invalidateAll();
		} catch (err) {
			problem = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<section id="integrity-settings" class="mt-6 space-y-5">
	<div class="panel p-5">
		<h3 class="text-lg font-semibold text-white">
			{lang === 'zh' ? '风控设置' : 'Integrity settings'}
		</h3>
		<p class="mt-1 text-sm text-mist-400">
			{lang === 'zh'
				? '组织所有者可以调整阈值与权重。改动记入审计日志并生成规则版本。自动处置由独立的实验性开关控制，默认关闭。'
				: 'Organisation owners can adjust thresholds and weights. Changes are audited and versioned. Separate experimental switches control automatic actions and are off by default.'}
		</p>
		<p class="mt-2 text-xs text-warn">
			{lang === 'zh'
				? '数据接入状态：180 秒步兵 KPM、独立受害者、爆头率、穿透率、游戏时钟短时爆发、独立举报人数与重复高风险窗口已接入；Steam VAC / 游戏封禁仅在缓存有效且查询成功时计分。WARDOGS 官方总游戏时间与游戏聊天接收未接入。'
				: 'Data status: infantry KPM, unique victims, headshot rate, penetration rate, game-clock bursts, unique reporters and repeat high-risk windows are connected. Steam VAC/game bans count only with valid lookup data. Official WARDOGS playtime and inbound game chat are unavailable.'}
		</p>
		<div
			class="mt-4 grid gap-2 sm:grid-cols-5"
			aria-label={lang === 'zh' ? '当前风险等级区间预览' : 'Current risk level ranges'}
		>
			{#each ranges as range (range.zh)}
				<div class="rounded-ctl border border-white/10 p-3">
					<div class="font-mono text-lg text-white">{range.start}–{range.end}</div>
					<div class="text-xs text-mist-300">{lang === 'zh' ? range.zh : range.en}</div>
				</div>
			{/each}
		</div>
		<p class="mt-2 text-xs text-mist-400">
			{lang === 'zh'
				? '区间会随输入实时更新。分数只决定处置资格；实际自动处置还要求对应开关开启且通过实时安全检查。'
				: 'Ranges update as you edit. Scores determine eligibility; automatic actions also require the matching switch and live safety checks.'}
		</p>
		<h4 class="mt-5 text-sm font-semibold text-white">
			{lang === 'zh' ? '180 秒纯步兵 KPM 分段' : '180-second infantry KPM bands'}
		</h4>
		<p class="mt-1 text-xs text-mist-400">
			{lang === 'zh'
				? '每档从本档起点（含）到下一档起点（不含）；只按命中的最高一档加分，不叠加。'
				: 'Each band runs from its threshold (inclusive) to the next (exclusive). Only the highest matching band adds points.'}
		</p>
		<div class="mt-2 grid gap-2 sm:grid-cols-5">
			{#each draft.kpmBands as band, i (i)}
				{@const nextMin = draft.kpmBands[i + 1]?.min}
				<div class="rounded-ctl border border-white/10 p-3 text-xs text-mist-300">
					<div class="font-semibold text-white">
						{lang === 'zh' ? `第 ${i + 1} 档` : `Band ${i + 1}`}
					</div>
					<div class="mt-1 min-h-8 font-mono text-accent">
						{#if nextMin === undefined}
							KPM ≥ {Number(band.min).toFixed(2)}
						{:else if nextMin > band.min}
							{Number(band.min).toFixed(2)} ≤ KPM &lt; {Number(nextMin).toFixed(2)}
						{:else}
							{lang === 'zh' ? '门槛必须递增' : 'Thresholds must increase'}
						{/if}
					</div>
					<label class="mt-2 block">
						<span>{lang === 'zh' ? '起点 KPM' : 'Minimum KPM'}</span>
						<input
							class="mt-1 input w-full"
							type="number"
							min="1"
							max="20"
							step="0.1"
							bind:value={band.min}
						/>
					</label>
					<label class="mt-2 block">
						<span>{lang === 'zh' ? '本档加分' : 'Points for this band'}</span>
						<input
							class="mt-1 input w-full"
							type="number"
							min="0"
							max="100"
							step="1"
							bind:value={band.points}
						/>
					</label>
				</div>
			{/each}
		</div>
		<p class="mt-2 text-xs text-mist-400">
			{lang === 'zh'
				? '低于第一档视为正常 KPM；KPM 与 KD 不会单独构成作弊定论。'
				: 'Below the first band is normal KPM. KPM and KD alone do not establish cheating.'}
		</p>
		<div class="mt-5 grid gap-5 lg:grid-cols-3">
			{#each fields as group (group.headingEn)}
				<div>
					<h4 class="mb-2 text-sm font-semibold text-white">
						{lang === 'zh' ? group.headingZh : group.headingEn}
					</h4>
					<div class="space-y-2">
						{#each group.entries as field (field.key)}
							<label class="flex items-center justify-between gap-2 text-xs text-mist-300"
								><span>{lang === 'zh' ? field.zh : field.en}</span><input
									class="input w-24"
									type="number"
									min={field.min}
									max={field.max}
									step={field.step ?? 1}
									value={draft[field.key]}
									oninput={(event) => {
										(draft as Record<NumericKey, number>)[field.key] = Number(
											event.currentTarget.value
										);
									}}
								/></label
							>
						{/each}
					</div>
				</div>
			{/each}
		</div>
		<div class="mt-5 grid gap-5 lg:grid-cols-2">
			{#each [{ key: 'uniqueVictimBands', zh: '独立受害者加分', en: 'Unique victim points' }, { key: 'reportBands', zh: '独立举报人加分', en: 'Unique reporter points' }] as group (group.key)}
				<div>
					<h4 class="mb-2 text-sm font-semibold text-white">
						{lang === 'zh' ? group.zh : group.en}
					</h4>
					<div class="flex flex-wrap gap-2">
						{#each draft[group.key as 'uniqueVictimBands' | 'reportBands'] as band, i (i)}
							<label class="text-xs text-mist-300"
								>≥ <input class="input w-16" type="number" min="1" step="1" bind:value={band.min} />
								→ +
								<input
									class="input w-16"
									type="number"
									min="0"
									max="100"
									step="1"
									bind:value={band.points}
								/></label
							>
						{/each}
					</div>
				</div>
			{/each}
		</div>
		{#if validationError}<p class="mt-4 text-sm text-danger" role="alert">{validationError}</p>{/if}
		<button
			class="mt-5 btn btn-primary"
			type="button"
			disabled={busy || !!validationError}
			onclick={saveRules}>{lang === 'zh' ? '保存风控规则' : 'Save integrity rules'}</button
		>
		<button
			class="btn-quiet mt-5 ml-2 btn"
			type="button"
			disabled={busy}
			onclick={() => (draft = structuredClone(config))}
			>{lang === 'zh' ? '撤销未保存修改' : 'Discard unsaved changes'}</button
		>
		<button
			class="btn-quiet mt-5 ml-2 btn"
			type="button"
			disabled={busy}
			onclick={() => (draft = structuredClone(ruleDefaults))}
			>{lang === 'zh' ? '载入附件默认值' : 'Load default values'}</button
		>
	</div>

	<div class="panel p-5">
		<h3 class="text-lg font-semibold text-white">
			{lang === 'zh' ? '武器分类' : 'Weapon classification'}
		</h3>
		<p class="mt-1 text-sm text-mist-400">
			{lang === 'zh'
				? '输入 Kill Feed 中的完整 cause 标签。未识别的 UNKNOWN 不计入步兵 KPM。'
				: 'Enter an exact Kill Feed cause tag. UNKNOWN does not count toward infantry KPM.'}
		</p>
		<form
			class="mt-3 flex flex-wrap items-end gap-2"
			onsubmit={(event) => {
				event.preventDefault();
				void saveWeapon();
			}}
		>
			<label class="text-xs text-mist-300"
				>原因标签<input
					class="mt-1 input w-72 max-w-full"
					bind:value={cause}
					required
					maxlength="180"
					placeholder="Id.Item.WeaponName"
				/></label
			>
			<label class="text-xs text-mist-300"
				>{lang === 'zh' ? '分类' : 'Category'}<select class="mt-1 input" bind:value={category}
					>{#each categories as option (option)}<option value={option}>{option}</option
						>{/each}</select
				></label
			>
			<button class="btn btn-primary" type="submit" disabled={busy}
				>{lang === 'zh' ? '保存分类' : 'Save mapping'}</button
			>
		</form>
		{#if overrides.length}
			<div class="mt-4 table-wrap">
				<table>
					<thead
						><tr><th>原因标签</th><th>{lang === 'zh' ? '当前分类' : 'Category'}</th><th></th></tr
						></thead
					><tbody
						>{#each overrides as row (row.cause)}<tr
								><td class="font-mono">{row.cause}</td><td>{row.category}</td><td
									><button
										class="text-accent"
										type="button"
										disabled={busy}
										onclick={() => removeWeapon(row.cause)}
										>{lang === 'zh' ? '删除自定义' : 'Remove override'}</button
									></td
								></tr
							>{/each}</tbody
					>
				</table>
			</div>
		{:else}<p class="mt-4 text-sm text-mist-400">
				{lang === 'zh'
					? '尚无自定义分类；使用内置分类。'
					: 'No overrides; built-in mappings apply.'}
			</p>{/if}
		<details class="mt-4 text-xs text-mist-400">
			<summary class="cursor-pointer"
				>{lang === 'zh' ? '查看内置步兵武器标签' : 'Built-in infantry weapon tags'}</summary
			>
			<p class="mt-2 font-mono break-all">{Object.keys(weaponDefaults).join(' · ')}</p>
		</details>
	</div>
	{#if notice}<p class="text-sm text-green-400" role="status">{notice}</p>{/if}
	{#if problem}<p class="text-sm text-red-400" role="alert">{problem}</p>{/if}
</section>
