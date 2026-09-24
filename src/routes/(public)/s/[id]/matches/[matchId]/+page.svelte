<script lang="ts">
	// A public match page: the match panel as the dossier shows it, names linking to public
	// careers, and the match's kill feed as the live page shows kills (names and factions only).
	import { api, qs } from '$lib/api';
	import MatchPanel from '$lib/components/MatchPanel.svelte';
	import { causeLabel } from '$lib/causes';
	import { factionColor, fmtDuration, mapName } from '$lib/format';
	import type { PublicKill } from '$lib/server/public';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let base = $derived(`/s/${encodeURIComponent(data.heading.id)}`);
	let view = $derived(data.match);
	// Older kills pile up under the load's first page; another match starts over.
	let extra = $state<PublicKill[]>([]);
	let extraMore = $state<boolean | null>(null);
	let more = $derived(extraMore ?? data.more);
	let loading = $state(false);
	let feed = $derived([...data.feed, ...extra]);
	$effect(() => {
		void view.match.id;
		extra = [];
		extraMore = null;
	});

	async function older() {
		const last = feed[feed.length - 1];
		if (!last) return;
		loading = true;
		try {
			const r = await api<{ feed: PublicKill[]; more: boolean }>(
				'GET',
				`/api/public/servers/${encodeURIComponent(data.heading.id)}/matches/${view.match.id}${qs({
					before: last.ts,
					beforeTime: last.eventTime
				})}`
			);
			extra = [...extra, ...r.feed];
			extraMore = r.more;
		} catch {
			/* the page keeps what it has */
		} finally {
			loading = false;
		}
	}
	const colorOf = (faction: string | null) =>
		view.factions.find((f) => f.name === faction)?.colorHex || factionColor(faction);
	const causeText = (k: PublicKill) =>
		causeLabel(k.cause) || (k.tags.includes('Falling') ? 'Fall' : '—');
</script>

<svelte:head>
	<title>{mapName(view.match.map)} · {data.heading.name} · {data.appName}</title>
	<meta name="description" content="{data.heading.name} 的对局：{mapName(view.match.map)}。" />
</svelte:head>

<div class="rise">
	<a href="{base}/matches" class="caps text-mist-400 hover:text-mist-100">← 比赛记录</a>
	<div class="mt-3 panel">
		<MatchPanel {view} hrefFor={(steamId) => `${base}/players/${steamId}`} />
		{#if view.kills}
			<span class="mt-4 field-label">击杀事件 · {view.kills}</span>
			<div class="table-wrap">
				<table>
					<thead>
						<tr><th>时钟</th><th>击杀者</th><th>受害者</th><th>与</th><th class="num">范围</th></tr>
					</thead>
					<tbody>
						{#each feed as k (k.eventId)}
							<tr>
								<td class="font-mono text-[12px] whitespace-nowrap text-mist-400"
									>{fmtDuration(k.eventTime)}</td
								>
								<td>
									{#if k.killer}
										<span style="color:{colorOf(k.killer.faction)}">{k.killer.name}</span>
									{:else}<span class="text-mist-600">—</span>{/if}
								</td>
								<td>
									<span style="color:{colorOf(k.victim.faction)}">{k.victim.name}</span>
									{#if k.teamKill}<span class="chip">误杀队友</span>{/if}
									{#if k.suicide}<span class="chip">自杀</span>{/if}
									{#if k.headshot}<span class="chip">爆头</span>{/if}
								</td>
								<td class="text-mist-400">{causeText(k)}</td>
								<td class="num text-mist-400"
									>{k.distanceM === null ? '—' : `${Math.round(k.distanceM)} m`}</td
								>
							</tr>
						{:else}
							<tr
								><td colspan="5" class="py-4 text-center text-mist-600"
									>尚未收到本场比赛的击杀记录。</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			</div>
			{#if more}
				<button class="mt-3 btn" onclick={older} disabled={loading}>
					{loading ? 'Loading…' : '加载更早的击杀记录'}
				</button>
			{/if}
		{/if}
	</div>
</div>
