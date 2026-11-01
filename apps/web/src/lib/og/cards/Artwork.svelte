<script lang="ts">
	import { initials } from '#lib/format.ts';
	import { monogramSwatch } from '#lib/ui/Monogram.svelte';

	/** A card's square picture (an avatar, a mix's cover), or the name's monogram when there's none. */
	let { src, name, size }: { src: string | null; name: string; size: number } = $props();

	const tile = $derived(monogramSwatch(name));

	const box = $derived(`width: ${size}px; height: ${size}px; border-radius: ${Math.round(size / 15)}px; flex-shrink: 0;`);
</script>

{#if src}
	<img {src} alt="" width={size} height={size} style="{box} object-fit: cover;" />
{:else}
	<div style="{box} display: flex; align-items: center; justify-content: center; background-color: {tile.background}; color: {tile.ink}; font-size: {Math.round(size * 0.36)}px; font-weight: 600; letter-spacing: -0.02em;">{initials(name)}</div>
{/if}
