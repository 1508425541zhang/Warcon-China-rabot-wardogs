<script lang="ts">
	import { fmtTime } from '$lib/format';
	import type { CashPoint } from '$lib/player-progress';
	type Round = {
		id: number;
		map: string;
		startedAt: string;
		endedAt: string | null;
		truncated: boolean;
		points: CashPoint[];
	};
	let { rounds }: { rounds: Round[] } = $props();
	let selected = $state(0);
	let round = $derived(rounds.find((r) => r.id === selected) ?? rounds[0]);
	const values = (points: CashPoint[], mode: string) =>
		points.flatMap((p) => (mode === 'cash' ? [p.cash] : [p.growth, p.serverGrowth]));
	const limits = (points: CashPoint[], mode: string) => ({
		lo: Math.min(0, ...values(points, mode)),
		hi: Math.max(1, ...values(points, mode)),
		end: Math.max(1, ...points.map((p) => p.seconds))
	});
	const x = (v: number, end: number) => 55 + (590 * v) / end;
	const y = (v: number, lo: number, hi: number) => 190 - (140 * (v - lo)) / (hi - lo);
	function segments(points: CashPoint[], field: 'cash' | 'growth' | 'serverGrowth', mode: string) {
		const l = limits(points, mode);
		const groups: string[][] = [];
		let last = -Infinity;
		for (const p of points) {
			if (p.seconds - last > 120) groups.push([]);
			groups.at(-1)!.push(`${x(p.seconds, l.end)},${y(p[field], l.lo, l.hi)}`);
			last = p.seconds;
		}
		return groups.map((g) => g.join(' '));
	}
</script>

<section class="mb-4 panel p-4">
	<h3 class="text-base font-semibold text-white">对局金钱与经验增长 · 管理员参考</h3>
	<p class="mt-2 text-xs text-mist-400">
		只使用真实采样，不参与反作弊判断。金钱是服务器记分板当前值，支出或重置可能使曲线下降；净增长以每位玩家首次记录为起点，不能等同于总收入。全服线是该采样时在线玩家的平均净增长（含本人），人数变动也会影响均值。
	</p>
	<p class="mt-1 text-xs text-mist-400">
		从本次上线后开始记录，每 30 秒最多保存一次。未监测到的开局、离线时段及历史过程不补画；超过 2
		分钟的缺口断开显示。最近 30 天最多列出 10 场。
	</p>
	{#if round}
		<label class="mt-3 flex items-center gap-3 text-sm"
			>⇄ 切换对局 <select
				class="rounded border border-white/20 bg-ink-900 px-3 py-2"
				value={round.id}
				onchange={(e) => (selected = Number(e.currentTarget.value))}
				>{#each rounds as r (r.id)}<option value={r.id}
						>{r.map} · {fmtTime(r.startedAt)} · {r.endedAt ? '已结束' : '进行中'}</option
					>{/each}</select
			></label
		>
		<p class="mt-2 text-xs text-mist-400">
			{round.points.length} 个本人采样点 · {round.truncated
				? '记录达到读取上限，仅展示前段'
				: '已保存的实际观测区间'} · 结束：{round.endedAt ? fmtTime(round.endedAt) : '尚未结束'}
		</p>
		{#each ['cash', 'growth'] as mode}
			{@const l = limits(round.points, mode)}
			<h4 class="mt-4 text-sm font-semibold">
				{mode === 'cash' ? '本人金钱变化曲线' : '金钱净增长：本人 vs 全服平均'}
			</h4>
			{#if round.points.length >= 2}
				<svg
					class="w-full"
					viewBox="0 0 700 235"
					role="img"
					aria-label={mode === 'cash' ? '本人在本场的金钱变化' : '本人和全服平均金钱净增长比较'}
				>
					<title>{mode === 'cash' ? '本人金钱' : '金钱净增长对比'}（服务器记分板单位）</title>
					{#each [0, 0.5, 1] as tick}<line
							x1="55"
							x2="645"
							y1={190 - 140 * tick}
							y2={190 - 140 * tick}
							stroke="currentColor"
							opacity="0.12"
						/><text x="48" y={194 - 140 * tick} text-anchor="end" fill="currentColor" font-size="11"
							>{Math.round(l.lo + (l.hi - l.lo) * tick)}</text
						>{/each}
					{#each segments(round.points, mode === 'cash' ? 'cash' : 'growth', mode) as path}<polyline
							points={path}
							fill="none"
							stroke="#fbbf24"
							stroke-width="2.5"
						/>{/each}
					{#if mode === 'growth'}{#each segments(round.points, 'serverGrowth', mode) as path}<polyline
								points={path}
								fill="none"
								stroke="#38bdf8"
								stroke-width="2"
							/>{/each}{/if}
					{#each round.points as p}<circle
							cx={x(p.seconds, l.end)}
							cy={y(mode === 'cash' ? p.cash : p.growth, l.lo, l.hi)}
							r="2"
							fill="#fbbf24"
							><title
								>{(p.seconds / 60).toFixed(1)} 分钟 · 本人金钱 {p.cash} · 本人净增长 {p.growth} · 全服平均净增长
								{p.serverGrowth.toFixed(1)} · 在线样本 {p.peers} 人</title
							></circle
						>{/each}
					{#each [0, 0.25, 0.5, 0.75, 1] as tick}<text
							x={55 + 590 * tick}
							y="208"
							text-anchor="middle"
							fill="currentColor"
							font-size="11">{((l.end * tick) / 60).toFixed(1)}</text
						>{/each}
					<text x="350" y="230" text-anchor="middle" fill="currentColor" font-size="11"
						>距本场记录开始（分钟）</text
					>
				</svg>
			{:else}<p class="mt-2 text-sm text-mist-400">
					采样中：至少需要两个时间点才能绘制增长曲线。
				</p>{/if}
		{/each}
		<p class="text-xs text-mist-400">
			<span style="color:#fbbf24">黄线：本人</span> ·
			<span style="color:#38bdf8">蓝线：全服平均净增长</span>
		</p>
	{:else}<p class="mt-4 text-sm text-mist-400">
			暂无对局过程采样。玩家在线且服务器成功返回状态与玩家列表后会开始记录；旧对局只有汇总数据，不能还原增长曲线。
		</p>{/if}
	<div class="mt-4 rounded border border-white/10 p-3">
		<h4 class="text-sm font-semibold">经验增长：本人 vs 全服</h4>
		<p class="mt-1 text-xs text-mist-400">
			不可用：当前游戏接口未提供玩家经验值。接口中的 experiences
			是游戏模式，不是经验。不会用击杀、金钱或模式名称代替经验曲线。
		</p>
	</div>
</section>
