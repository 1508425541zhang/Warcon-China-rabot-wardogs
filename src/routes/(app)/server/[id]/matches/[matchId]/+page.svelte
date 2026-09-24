<script lang="ts">
	// One match: the header, final scores, the score over time, awards and the scoreboard from the
	// page's data, and the match's kill feed from the kills route narrowed to it, paged.
	import { api, errorMessage, qs } from '$lib/api';
	import { toast } from '$lib/toast.svelte';
	import MatchPanel from '$lib/components/MatchPanel.svelte';
	import FactionChip from '$lib/components/FactionChip.svelte';
	import { causeLabel } from '$lib/causes';
	import { fmtDuration } from '$lib/format';
	import type { KillView } from '$lib/types';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let id = $derived(data.server.id);
	let view = $derived(data.match);
	const PAGE = 100;
	let kills = $state<KillView[]>([]);
	let more = $state(false);
	let loading = $state(false);
	let seq = 0;

	async function load(append = false) {
		const my = ++seq;
		const last = append ? kills[kills.length - 1] : undefined;
		loading = true;
		try {
			const r = await api<{ kills: KillView[] }>(
				'GET',
				`/api/servers/${encodeURIComponent(id)}/kills${qs({
					match: view.match.id,
					limit: PAGE,
					before: last?.ts,
					beforeTime: last?.eventTime
				})}`
			);
			if (my !== seq) return;
			kills = last ? [...kills, ...r.kills] : r.kills;
			more = r.kills.length === PAGE;
		} catch (err) {
			toast(errorMessage(err), 'err');
		} finally {
			if (my === seq) loading = false;
		}
	}
	$effect(() => {
		void view.match.id;
		kills = [];
		if (view.kills) void load();
	});
	const dossier = (steamId: string) =>
		`/server/${encodeURIComponent(id)}/players/${encodeURIComponent(steamId)}`;
	const factions = $derived(
		view.factions.map((f) => ({ name: f.name, colorHex: f.colorHex ?? '', score: 0 }))
	);
	const causeText = (k: KillView) =>
		causeLabel(k.cause) || (k.tags.includes('Falling') ? 'Fall' : '—');
</script>

<div class="panel">
	<a href="/server/{encodeURIComponent(id)}/matches" class="caps text-mist-400 hover:text-mist-100"
		>← 比赛记录</a
	>
	<div class="mt-3">
		<MatchPanel {view} hrefFor={dossier} showIds />
	</div>
	{#if view.kills}
		<span class="mt-4 field-label">击杀事件 · {view.kills}</span>
		<div class="table-wrap">
			<table>
				<thead>
					<tr
						><th>时钟</th><th>击杀者</th><th>受害者</th><th>击杀原因</th><th class="num">距离</th
						><th></th></tr
					>
				</thead>
				<tbody>
					{#each kills as k (k.eventId)}
						<tr class={k.teamKill ? 'text-warn' : ''}>
							<td class="font-mono text-[12px] whitespace-nowrap text-mist-400"
								>{fmtDuration(k.eventTime)}</td
							>
							<td>
								{#if k.killer}
									<a href={dossier(k.killer.steamId)} class="hover:text-accent hover:underline"
										>{k.killer.name}</a
									>
									{#if k.killer.faction}<FactionChip
											faction={k.killer.faction}
											scores={factions}
										/>{/if}
								{:else}<span class="text-mist-600">—</span>{/if}
							</td>
							<td>
								<a href={dossier(k.victim.steamId)} class="hover:text-accent hover:underline"
									>{k.victim.name}</a
								>
								{#if k.victim.faction}<FactionChip
										faction={k.victim.faction}
										scores={factions}
									/>{/if}
							</td>
							<td class="text-mist-200">{causeText(k)}</td>
							<td class="num">{k.distanceM === null ? '—' : `${Math.round(k.distanceM)} m`}</td>
							<td class="whitespace-nowrap">
								{#if k.teamKill}<span class="chip">误杀队友</span>{/if}
								{#if k.suicide}<span class="chip">自杀</span>{/if}
								{#if k.headshot}<span class="chip">爆头</span>{/if}
							</td>
						</tr>
					{:else}
						<tr
							><td colspan="6" class="py-4 text-center text-mist-600"
								>{loading ? 'Loading…' : '尚未收到本场对局的击杀记录。'}</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		{#if more}
			<button class="mt-3 btn" onclick={() => load(true)} disabled={loading}>
				{loading ? 'Loading…' : '加载更早的击杀记录'}
			</button>
		{/if}
	{/if}
	<p class="note">
		击杀、死亡和现金来自游戏计分板在本场比赛中的累计值；时间为各玩家在本场的在线时长。 {#if view.hasFeed}爆头、误杀队友、载具击杀和连续击杀来自游戏击杀事件；下方按最新击杀优先显示。{:else}本服未启用击杀事件，因此无法显示相关列。{/if}
		颁发奖项要求比赛至少持续 20 分钟；最高 KD 奖要求至少 10 次击杀。
	</p>
</div>
