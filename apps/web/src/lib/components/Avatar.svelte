<script lang="ts">
	import Monogram from '#lib/ui/Monogram.svelte';

	/** A project's photo, or its monogram when there isn't one. Sizes match Monogram's. */
	let { src = null, name, size = 'md' }: { src?: string | null; name: string; size?: 'sm' | 'md' | 'lg' | 'xl' } = $props();

	const sizes = { sm: 'size-8 rounded-lg', md: 'size-10 rounded-lg', lg: 'size-11 rounded-lg', xl: 'size-16 rounded-xl' };
</script>

{#if src}
	<img {src} alt="" loading="lazy" class="{sizes[size]} shrink-0 bg-well object-cover" />
{:else}
	<Monogram {name} {size} />
{/if}
