<script lang="ts">
	import {
		awardKeys,
		awardLabels,
		awardsSchema,
		defaultAwards,
		type AwardsConfig
	} from '$lib/match-awards-policy';
	let { awards = $bindable<AwardsConfig>(), endMessage = $bindable<string>() } = $props();
	let message = $state('');
	async function upload(event: Event) {
		const input = event.target as HTMLInputElement,
			file = input.files?.[0];
		if (!file) return;
		try {
			if (file.size > 16384) throw new Error('模板文件不能超过16KB');
			const text = await file.text();
			if (file.name.toLowerCase().endsWith('.json')) {
				const parsed = awardsSchema.safeParse(JSON.parse(text));
				if (!parsed.success)
					throw new Error('JSON必须包含enabled和六个奖项的templates字段，每条最多200字符');
				awards = parsed.data;
			} else {
				if (text.trim().length > 200) throw new Error('文字公告最多200字符；多奖项请用JSON模板');
				endMessage = text.trim();
			}
			message = '已导入编辑框，点击规则保存后才会生效';
		} catch (e) {
			message = e instanceof Error ? e.message : '读取失败';
		}
		input.value = '';
	}
	function download() {
		const url = URL.createObjectURL(
			new Blob([JSON.stringify(awards, null, 2)], { type: 'application/json' })
		);
		const a = document.createElement('a');
		a.href = url;
		a.download = '赛后荣誉模板.json';
		a.click();
		URL.revokeObjectURL(url);
	}
</script>

<fieldset class="space-y-3 rounded border border-mist-700 p-4">
	<legend class="field-label">赛后自动荣誉公告</legend>
	<label><input type="checkbox" bind:checked={awards.enabled} /> 自动生成本局获奖名单并广播</label>
	<p class="note">
		每项只选第一名；并列随机选一人，结果随本局快照保存。留空模板可关闭单个奖项。KD=击杀÷max(死亡,1)，无击杀不参与MVP。
	</p>
	{#each awardKeys as key}<label class="block"
			>{awardLabels[key]}<input
				class="mt-1 input"
				type="text"
				maxlength="200"
				bind:value={awards.templates[key]}
			/></label
		>{/each}
	<p class="note">
		奖项占位符：&#123;name&#125;、&#123;value&#125;、&#123;kills&#125;、&#123;deaths&#125;、&#123;previous&#125;。单次多杀按同一玩家同一游戏时间戳统计（至少2杀）；一命连杀在死亡时清零。金钱净增并非消费前总收入；最富按赛末最近持有金额。在线时长按已观测记录。
	</p>
	<p class="note">
		工程／医疗最佳贡献：当前接口没有贡献分，暂不自动颁发。数据不足的奖项跳过，不编造获奖者。击杀日志未覆盖的部分无法恢复。
	</p>
	<label class="block"
		>上传文字／JSON模板<input
			class="mt-2 block"
			type="file"
			accept=".txt,.json,text/plain,application/json"
			onchange={upload}
		/></label
	>
	<div class="flex flex-wrap gap-2">
		<button
			class="btn"
			type="button"
			onclick={() => (awards = structuredClone({ ...defaultAwards, enabled: true }))}
			>使用自动生成模板</button
		><button class="btn" type="button" onclick={download}>下载当前JSON模板</button>
	</div>
	{#if message}<p role="status">{message}</p>{/if}
</fieldset>
