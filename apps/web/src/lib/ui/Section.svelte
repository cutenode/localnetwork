<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A full-width band of the page with a rule under it. Alternate `canvas` and `sunken` down a
	 * long page; `grid` is for the hero and the closing call.
	 */
	let {
		id,
		labelledby,
		tone = 'canvas',
		space = 'lg',
		children
	}: { id?: string; labelledby?: string; tone?: 'canvas' | 'sunken' | 'grid'; space?: 'none' | 'md' | 'lg'; children: Snippet } = $props();
</script>

<section {id} aria-labelledby={labelledby} class={['border-b border-line', tone === 'sunken' ? 'bg-sunken' : 'bg-canvas', tone === 'grid' && 'bg-grid']}>
	<div class={['container-page', { none: '', md: 'py-16', lg: 'py-20 sm:py-30' }[space]]}>
		{@render children()}
	</div>
</section>
