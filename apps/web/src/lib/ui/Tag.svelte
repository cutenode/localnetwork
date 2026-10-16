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

	const classes = $derived([
		'inline-flex items-center gap-1.5 rounded-full border no-underline transition-colors',
		variant === 'mono' ? 'px-2 py-px font-mono text-caption text-fg-muted' : 'px-2.5 py-0.5 text-label',
		pressed ? 'border-fg bg-fg font-semibold text-fg-inverse' : 'border-line-strong text-fg',
		(href || pressed !== undefined) && !pressed && 'hover:border-fg-subtle',
		pressed !== undefined && 'min-h-9 px-3'
	]);
</script>

{#if href}
	<a {href} class={classes}>{@render children()}</a>
{:else if pressed !== undefined}
	<button type="button" aria-pressed={pressed} {onclick} class={classes}>{@render children()}</button>
{:else}
	<span class={classes}>{@render children()}</span>
{/if}
