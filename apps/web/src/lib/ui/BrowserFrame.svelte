<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A browser window around a product screen. `bleed` drops the bottom edge so the window runs
	 * off the bottom of its section, as in the hero.
	 */
	let { url, bleed = false, children }: { url: string; bleed?: boolean; children: Snippet } = $props();
</script>

<figure class={['m-0 overflow-hidden border border-line-strong bg-surface text-left shadow-glow', bleed ? 'rounded-t-xl border-b-0' : 'rounded-xl']}>
	<figcaption class="flex items-center gap-2 border-b border-line px-4 py-3">
		{#each [1, 2, 3] as dot (dot)}<span aria-hidden="true" class="size-2.5 rounded-full bg-line-strong"></span>{/each}
		<span class="mx-auto truncate rounded-sm bg-well px-3.5 py-1 font-mono text-caption text-fg-subtle">{url}</span>
	</figcaption>
	{@render children()}
</figure>
