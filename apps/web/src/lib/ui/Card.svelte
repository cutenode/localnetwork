<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';

	/**
	 * A bordered panel. With `href` the whole card is one link. `surface` sits on canvas,
	 * `outline` is for cards inside a surface, `well` for an inset block.
	 */
	let {
		as = 'div',
		href,
		tone = 'surface',
		padding = 'md',
		radius = 'xl',
		class: extra,
		children
	}: {
		as?: 'div' | 'article' | 'li' | 'section' | 'aside';
		href?: string;
		tone?: 'surface' | 'outline' | 'well';
		padding?: 'none' | 'sm' | 'md' | 'lg';
		radius?: 'lg' | 'xl' | '2xl';
		class?: ClassValue;
		children: Snippet;
	} = $props();

	const classes = $derived([
		{ surface: 'border border-line bg-surface', outline: 'border border-line', well: 'bg-well' }[tone],
		{ none: '', sm: 'p-4', md: 'p-5', lg: 'p-6' }[padding],
		{ lg: 'rounded-lg', xl: 'rounded-xl', '2xl': 'rounded-2xl' }[radius],
		href && 'block text-fg no-underline transition-colors hover:border-line-strong',
		extra
	]);
</script>

{#if href}
	<a {href} class={classes}>{@render children()}</a>
{:else}
	<svelte:element this={as} class={classes}>{@render children()}</svelte:element>
{/if}
