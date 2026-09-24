<script lang="ts">
	// A player's career: rank, streak, results overall, the scoreboard totals, by map and by
	// faction, and the last ten matches. Shared by the dossier's Career section and the public
	// career page; `matchHref` says where a match's name goes (its page here or on the public site).
	import type { Snippet } from 'svelte';
	import { fmtCash } from '$lib/cash';
	import { fmtNum, fmtTime, mapName } from '$lib/format';
	import { kdRatio, type CareerMatch, type CareerView } from '$lib/leaderboard';
	import { fmtLength, perMinute } from '$lib/matches';

	let {
		career,
		serverName,
		orgName,
		/** more than one server in the org: name the server on each match */
		multiServer = false,
		matchHref,
		/** rendered between the rank tiles and the tables (the public career puts combat here) */
		children
	}: {
		career: CareerView;
		serverName: string;
		orgName: string;
		multiServer?: boolean;
		matchHref?: (m: CareerMatch) => string;
		children?: Snippet;
	} = $props();

	const kd = (k: number, d: number) => {
		const v = kdRatio(k, d);
		return v === null ? '—' : v.toFixed(2);
	};
	const rank = (n: number | null) => (n === null ? '—' : `#${fmtNum(n)}`);
	const pct = (part: number, whole: number) =>
		whole && part ? `${Math.round((part / whole) * 100)}%` : '—';
	const kpm = (kills: number, minutes: number) => {
		const v = perMinute(kills, minutes * 60);
		return v === null ? '—' : v.toFixed(2);
	};
	const RESULT_TONE = { win: 'text-ok', loss: 'text-danger', draw: 'text-mist-400' } as const;
	const RESULT_LABEL = { win: '胜', loss: '负', draw: '平' } as const;
	const cash = (n: number) => `${n > 0 ? '+' : ''}${fmtCash(n)}`;
</script>

<div class="mb-4 grid grid-cols-2 gap-3 md:grid-cols-4">
	{#each [['本服排名', rank(career.rank.server)], [`“${orgName}”组织排名`, rank(career.rank.org)], ['近期连胜/连败', career.streak ? `${career.streak.n} 场${career.streak.kind === 'win' ? '连胜' : '连败'}` : '—'], ['对局（胜-负-平）', `${fmtNum(career.matches)} · ${career.wins}-${career.losses}-${career.draws}`], ['KD', `${kd(career.kills, career.deaths)} · ${fmtNum(career.kills)} / ${fmtNum(career.deaths)}`], ['每分钟击杀数', kpm(career.kills, career.minutes)], ['爆头率', pct(career.headshots, career.kills)], ['最佳连续击杀', career.killStreak ? `${career.killStreak} 次击杀` : '—']] as [label, value] (label)}
		<div class="rounded-ctl border border-black bg-ink-950 px-3.5 py-3">
			<div class="caps text-mist-400">{label}</div>
			<div class="mt-1 font-display text-xl font-semibold tabular">{value}</div>
		</div>
	{/each}
</div>
<p class="mb-3 text-[12.5px] text-mist-600">
	排名按所有服务器的历史总击杀数计算，且至少游玩
	{career.rank.floorMinutes} 分钟；未达到门槛时显示短横线。对局记录按胜、负、平显示。击杀、死亡和每分钟击杀数来自已结束对局的游戏计数器；爆头率和最佳连续击杀来自击杀事件。
</p>
{#if children}{@render children()}{/if}
<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
	<div>
		<span class="field-label">按地图</span>
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>地图</th><th class="num">比赛记录</th><th class="num">W</th><th class="num">K/D</th
						></tr
					></thead
				>
				<tbody>
					{#each career.maps as m (m.key)}
						<tr>
							<td>{mapName(m.key)}</td><td class="num">{m.matches}</td><td class="num">{m.wins}</td>
							<td class="num">{kd(m.kills, m.deaths)}</td>
						</tr>
					{:else}
						<tr><td colspan="4" class="py-4 text-center text-mist-600">暂无比赛记录。</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
	<div>
		<span class="field-label">按阵营</span>
		<div class="table-wrap">
			<table>
				<thead
					><tr
						><th>阵营</th><th class="num">比赛记录</th><th class="num">W</th><th class="num">K/D</th
						></tr
					></thead
				>
				<tbody>
					{#each career.factions as f (f.key)}
						<tr>
							<td>{f.key}</td><td class="num">{f.matches}</td><td class="num">{f.wins}</td>
							<td class="num">{kd(f.kills, f.deaths)}</td>
						</tr>
					{:else}
						<tr><td colspan="4" class="py-4 text-center text-mist-600">暂无比赛记录。</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
</div>
{#if career.last.length}
	<span class="mt-4 field-label"
		>最近 {career.last.length === 1 ? 'match' : `${career.last.length} matches`}</span
	>
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<th>开始时间</th>{#if multiServer}<th>服务器</th>{/if}<th>地图</th><th>阵营</th><th
						>结果</th
					>
					<th class="num">时间</th><th class="num">K</th><th class="num">D</th><th class="num"
						>现金</th
					>
				</tr>
			</thead>
			<tbody>
				{#each career.last as m (m.matchId)}
					<tr>
						<td class="whitespace-nowrap">{fmtTime(m.startedAt)}</td>
						{#if multiServer}<td>{m.serverName}</td>{/if}
						<td>
							{#if matchHref}<a href={matchHref(m)} class="hover:text-accent hover:underline"
									>{m.map ? mapName(m.map) : 'Match'}</a
								>{:else}{m.map ? mapName(m.map) : '—'}{/if}
						</td>
						<td>{m.faction || '—'}</td>
						<td class={m.result ? RESULT_TONE[m.result] : 'text-mist-600'}
							>{m.result ? RESULT_LABEL[m.result] : m.endedAt ? '—' : '进行中'}</td
						>
						<td class="num whitespace-nowrap">{fmtLength(m.seconds)}</td>
						<td class="num">{m.kills}</td><td class="num">{m.deaths}</td>
						<td class="num {m.cashDelta > 0 ? 'text-ok' : m.cashDelta < 0 ? 'text-danger' : ''}"
							>{cash(m.cashDelta)}</td
						>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="note">
		对局在以下服务器结束后才计入： {serverName === orgName
			? '该服务器'
			: 'a server'}；其中的击杀和死亡来自游戏计分板。
	</p>
{/if}
