<script lang="ts">
	import type { Snippet } from 'svelte';
	import { CARD } from '../style.ts';

	/** A card scaled down to the width it's given, as a chat app shows a link preview. For the stories. */
	let { children }: { children: Snippet } = $props();

	let width = $state<number>(CARD.width);

	const scale = $derived(Math.min(1, width / CARD.width));
</script>

<div bind:clientWidth={width} style="width: 100%; max-width: {CARD.width}px; height: {CARD.height * scale}px; overflow: hidden; border-radius: 12px;">
	<div style="width: {CARD.width}px; transform: scale({scale}); transform-origin: top left;">{@render children()}</div>
</div>
