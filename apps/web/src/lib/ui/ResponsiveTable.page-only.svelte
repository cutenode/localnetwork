<script lang="ts" generics="Row">
	import type { Snippet } from 'svelte';

	/**
	 * A table where its box has room for every column, and a card per row where it doesn't, so a
	 * phone never hides a column off to the side. `head` is the header row's cells, `row` a row's
	 * cells and `card` a row's card. A table of a few short columns can show from `xl` (36rem) rather
	 * than `3xl` (48rem).
	 */
	let {
		rows,
		key,
		head,
		row,
		card,
		from = '3xl'
	}: { rows: Row[]; key: (row: Row) => string; head: Snippet; row: Snippet<[Row]>; card: Snippet<[Row]>; from?: 'xl' | '3xl' } = $props();
</script>

<div class="@container">
	<div class={['card hidden overflow-hidden', from === 'xl' ? '@xl:block' : '@3xl:block']}>
		<table class="table-base">
			<thead><tr>{@render head()}</tr></thead>
			<tbody>
				{#each rows as item (key(item))}<tr>{@render row(item)}</tr>{/each}
			</tbody>
		</table>
	</div>
	<ul class={['flex flex-col gap-3', from === 'xl' ? '@xl:hidden' : '@3xl:hidden']}>
		{#each rows as item (key(item))}<li class="card flex flex-col gap-3 p-4">{@render card(item)}</li>{/each}
	</ul>
</div>
