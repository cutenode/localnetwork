<script lang="ts">
	import type { Snippet } from 'svelte';
	import mark from '#lib/assets/favicon.svg?raw';
	import { CARD, ink, MONO, SANS } from '../style.ts';

	/**
	 * A card's canvas and its footer, the site's mark and name, small. `background` goes under everything, edge to
	 * edge (a press photo); the footer then sits on it.
	 */
	let { children, background }: { children: Snippet; background?: Snippet } = $props();

	const markSrc = `data:image/svg+xml;base64,${btoa(mark)}`;
</script>

<div style="display: flex; flex-direction: column; position: relative; width: {CARD.width}px; height: {CARD.height}px; padding: 64px 72px 44px; background-color: {ink.canvas}; color: {ink.fg}; font-family: {SANS}; overflow: hidden;">
	{#if background}{@render background()}{/if}
	<!-- Positioned, so a browser paints them over the background as Satori does, in the order they're written. -->
	<div style="display: flex; flex-direction: column; flex-grow: 1; min-height: 0; position: relative;">{@render children()}</div>
	<div style="display: flex; align-items: center; justify-content: space-between; margin-top: 32px; position: relative;">
		<div style="display: flex; align-items: center;">
			<img src={markSrc} alt="" width="40" height="40" style="width: 40px; height: 40px; border-radius: 8px;" />
			<span style="margin-left: 14px; font-size: 26px; font-weight: 600; color: {ink.fg};">Local Network</span>
		</div>
		<span style="font-family: {MONO}; font-size: 22px; color: {ink.fgMuted};">localnetwork.computer</span>
	</div>
</div>
