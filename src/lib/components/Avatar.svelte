<script lang="ts">
	import { hueFor, initials } from '#lib/format.ts';

	let { src = null, name, size = 'md' }: { src?: string | null; name: string; size?: 'sm' | 'md' | 'lg' | 'xl' } = $props();

	const sizes = { sm: 'size-9 text-lg', md: 'size-14 text-2xl', lg: 'size-20 text-4xl', xl: 'size-28 sm:size-36 text-6xl' };

	// Fixed ANSI pairs, each with readable contrast between block and letters.
	const blocks = [
		'bg-ansi-cyan text-ansi-black',
		'bg-ansi-magenta text-ansi-white',
		'bg-ansi-blue text-ansi-yellow',
		'bg-ansi-green text-ansi-black',
		'bg-ansi-red text-ansi-white',
		'bg-ansi-brown text-ansi-white',
		'bg-ansi-bright-blue text-ansi-white'
	];

	const block = $derived(blocks[hueFor(name) % blocks.length]);
</script>

{#if src}
	<img {src} alt="" loading="lazy" class="{sizes[size]} shrink-0 border-2 border-ansi-cyan object-cover" />
{:else}
	<div aria-hidden="true" class="{sizes[size]} {block} grid shrink-0 place-items-center leading-none">{initials(name)}</div>
{/if}
