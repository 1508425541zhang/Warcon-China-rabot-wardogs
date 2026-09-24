<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { api, errorMessage } from '$lib/api';
	import type { PageData } from './$types';

	type Rules = NonNullable<PageData['ruleConfig']>;
	type NumericKey = {
		[K in keyof Rules]: Rules[K] extends number ? K : never;
	}[keyof Rules];
	type Field = { key: NumericKey; zh: string; en: string; min: number; max: number; step?: number };
	let {
		orgId,
		config,
		overrides,
		defaults,
		categories,
		lang
	}: {
		orgId: string;
		config: Rules;
		overrides: PageData['weaponOverrides'];
		defaults: PageData['weaponDefaults'];
		categories: PageData['weaponCategories'];
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
				},
				{ key: 'quarantineDays', zh: '隔离期限（天）', en: 'Quarantine days', min: 1, max: 3650 }
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
		busy = true;
		problem = notice = '';
		try {
			await api('PUT', `/api/orgs/${encodeURIComponent(orgId)}/integrity/rules`, { values: draft });
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
				? '组织所有者可以调整阈值与权重。所有改动记入审计日志并生成规则版本。当前仅记录，不会自动踢人或隔离。'
				: 'Organisation owners can adjust thresholds and weights. Changes are audited and versioned. Enforcement remains off.'}
		</p>
		<p class="mt-2 text-xs text-warn">
			{lang === 'zh'
				? '数据接入状态：步兵 KPM、独立受害者、独立举报人、独立异常窗口已接入；爆头率、穿透率、短时爆发、Steam 处罚与公开游戏时间仍等待可信数据源，当前不参与实时评分。'
				: 'Data status: infantry KPM, unique victims, unique reporters and independent windows are connected. Headshots, penetration, bursts, Steam bans and public playtime await trusted feeds and do not currently affect live scores.'}
		</p>
		<h4 class="mt-5 text-sm font-semibold text-white">
			{lang === 'zh' ? '180 秒纯步兵 KPM 分段' : '180-second infantry KPM bands'}
		</h4>
		<div class="mt-2 grid gap-2 sm:grid-cols-5">
			{#each draft.kpmBands as band, i (i)}
				<label class="text-xs text-mist-300"
					>{lang === 'zh' ? `第 ${i + 1} 档` : `Band ${i + 1}`}
					<span class="mt-1 flex items-center gap-1"
						><input
							class="input w-20"
							type="number"
							min="1"
							max="20"
							step="0.1"
							bind:value={band.min}
							aria-label="KPM minimum"
						/><span>→</span><input
							class="input w-20"
							type="number"
							min="0"
							max="100"
							step="1"
							bind:value={band.points}
							aria-label="Risk points"
						/></span
					>
				</label>
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
		<button class="mt-5 btn btn-primary" type="button" disabled={busy} onclick={saveRules}
			>{lang === 'zh' ? '保存风控规则' : 'Save integrity rules'}</button
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
				>cause<input
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
						><tr><th>cause</th><th>{lang === 'zh' ? '当前分类' : 'Category'}</th><th></th></tr
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
			<p class="mt-2 font-mono break-all">{Object.keys(defaults).join(' · ')}</p>
		</details>
	</div>
	{#if notice}<p class="text-sm text-green-400" role="status">{notice}</p>{/if}
	{#if problem}<p class="text-sm text-red-400" role="alert">{problem}</p>{/if}
</section>
