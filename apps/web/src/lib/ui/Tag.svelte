<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { MouseEventHandler } from 'svelte/elements';

	/**
	 * A genre, kind or filter. With `href` it links, with `pressed` it toggles (a filter chip),
	 * otherwise it's a label. `mono` is for record-ish labels like a set's kind.
	 */
	let {
		href,
		pressed,
		onclick,
		variant = 'outline',
		children
	}: { href?: string; pressed?: boolean; onclick?: MouseEventHandler<HTMLButtonElement>; variant?: 'outline' | 'mono'; children: Snippet } = $props();

	// The chip and chip-toggle classes live in theme.css, so a plain link or checkbox label matches.
	const classes = $derived([
		pressed === undefined ? 'chip' : 'chip-toggle',
		href && 'transition-colors hover:border-fg-subtle',
		variant === 'mono' && 'px-2 py-px font-mono text-caption text-fg-muted'
	]);
</script>

{#if href}
	<a {href} class={classes}>{@render children()}</a>
{:else if pressed !== undefined}
	<button type="button" aria-pressed={pressed} {onclick} class={classes}>{@render children()}</button>
{:else}
	<span class={classes}>{@render children()}</span>
{/if}
