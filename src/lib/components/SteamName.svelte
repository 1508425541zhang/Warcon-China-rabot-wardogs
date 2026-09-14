<script lang="ts">
	// A Steam persona beside a SteamID: avatar and name when the lookup knows them, nothing otherwise.
	import type { SteamProfile } from '$lib/steam-profiles';

	let { profile, class: cls = '' }: { profile: SteamProfile | null | undefined; class?: string } =
		$props();
</script>

{#if profile?.name}
	<span class="inline-flex min-w-0 items-center gap-1.5 {cls}">
		{#if profile.avatar}<img
				src={profile.avatar}
				alt=""
				class="size-5 shrink-0 rounded-sm"
				loading="lazy"
				referrerpolicy="no-referrer"
			/>{/if}
		<span class="truncate">{profile.name}</span>
	</span>
{/if}
