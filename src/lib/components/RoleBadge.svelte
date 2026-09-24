<script lang="ts">
	// A role name as a badge. Tone follows what the role started as (org owner / site owner /
	// built-in admin: accent; operator: info), so a renamed built-in keeps its colour and a custom
	// role reads neutral.
	import Badge from './Badge.svelte';
	let { role, builtin = null }: { role: string; builtin?: string | null } = $props();
	let key = $derived(builtin ?? role);
	let display = $derived(
		(
			{ owner: '所有者', admin: '管理员', operator: '操作员', viewer: '查看者' } as Record<
				string,
				string
			>
		)[role] ?? role
	);
</script>

<Badge tone={key === 'admin' || key === 'owner' ? 'accent' : key === 'operator' ? 'info' : ''}
	>{display}</Badge
>
