<script lang="ts">
	import { embedFrame } from '#lib/mix/links.ts';

	/** A platform's player for a mix, from `embedUrl`. `element` is the iframe, for its widget API (`#lib/mix/widgets.ts`). */
	let { id, src, title, element = $bindable() }: { id: string; src: string; title: string; element?: HTMLIFrameElement } = $props();

	// Read once, as the page renders: nothing on the site switches the theme under an open player.
	const theme = typeof document !== 'undefined' && document.documentElement.classList.contains('light') ? 'light' : 'dark';

	const frame = $derived(embedFrame(src, theme));
</script>

<!-- The border is the wrapper's, so the player gets its whole height: Mixcloud's drops its progress bar when it's any shorter. -->
<div class="overflow-hidden rounded-lg border border-line bg-well">
	<iframe bind:this={element} {id} {title} src={frame.src} height={frame.height} class="block w-full" loading="lazy" allow="autoplay; encrypted-media" referrerpolicy="strict-origin-when-cross-origin"></iframe>
</div>
