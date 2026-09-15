<script lang="ts" generics="K extends string">
	// A sortable column header: the label is a button that toggles the table's sort, with an arrow
	// for the active column. `num` right-aligns it like the numeric cells beneath.
	import type { Snippet } from 'svelte';
	import type { SortLike } from '$lib/table.svelte';

	let {
		sort,
		key,
		num = false,
		class: cls = '',
		title,
		children
	}: {
		sort: SortLike<K>;
		key: K;
		num?: boolean;
		class?: string;
		title?: string;
		children: Snippet;
	} = $props();
	let active = $derived(sort.key === key);
</script>

<th
	class={[num && 'num', cls]}
	aria-sort={active ? (sort.dir === 'asc' ? 'ascending' : 'descending') : undefined}
>
	<button
		type="button"
		class="th-sort"
		class:active
		{title}
		onclick={(e) => {
			e.stopPropagation();
			sort.toggle(key);
		}}
	>
		{@render children()}<span class="th-arrow" aria-hidden="true"
			>{active ? (sort.dir === 'asc' ? '↑' : '↓') : ''}</span
		>
	</button>
</th>
