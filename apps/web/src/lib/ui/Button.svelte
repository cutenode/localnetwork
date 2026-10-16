<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from 'svelte/elements';

	type Variant = 'primary' | 'secondary' | 'ghost';

	type Size = 'sm' | 'md' | 'lg';

	type Props = { variant?: Variant; size?: Size; children: Snippet } & ((HTMLAnchorAttributes & { href: string }) | (HTMLButtonAttributes & { href?: undefined }));

	let { variant = 'primary', size = 'md', children, class: extra, ...rest }: Props = $props();

	// The classes live in theme.css, so a plain <button class="btn-primary"> in a form matches.
	const variants: Record<Variant, string> = { primary: 'btn-primary', secondary: 'btn-secondary', ghost: 'btn-ghost' };

	const sizes: Record<Size, string> = { sm: 'btn-sm', md: '', lg: 'btn-lg' };

	const classes = $derived([variants[variant], sizes[size], extra]);
</script>

{#if rest.href === undefined}
	<button type="button" {...rest as HTMLButtonAttributes} class={classes}>{@render children()}</button>
{:else}
	<a {...rest as HTMLAnchorAttributes} class={classes}>{@render children()}</a>
{/if}
