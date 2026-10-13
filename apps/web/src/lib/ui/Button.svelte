<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from 'svelte/elements';

	type Variant = 'primary' | 'secondary' | 'ghost';

	type Size = 'sm' | 'md' | 'lg';

	type Props = { variant?: Variant; size?: Size; children: Snippet } & ((HTMLAnchorAttributes & { href: string }) | (HTMLButtonAttributes & { href?: undefined }));

	let { variant = 'primary', size = 'md', children, class: extra, ...rest }: Props = $props();

	const variants: Record<Variant, string> = {
		primary: 'bg-fg text-fg-inverse hover:bg-white',
		secondary: 'border border-line-strong bg-raised text-fg hover:border-fg-subtle',
		ghost: 'text-fg-muted hover:bg-raised hover:text-fg'
	};

	const sizes: Record<Size, string> = {
		sm: 'min-h-9 rounded-md px-3.5 text-small',
		md: 'min-h-11 rounded-lg px-4.5 text-body',
		lg: 'min-h-12 rounded-lg px-5.5 text-body'
	};

	const classes = $derived([
		'inline-flex items-center justify-center gap-2 font-semibold whitespace-nowrap no-underline transition-colors disabled:cursor-not-allowed disabled:opacity-50',
		variants[variant],
		sizes[size],
		extra
	]);
</script>

{#if rest.href === undefined}
	<button type="button" {...rest as HTMLButtonAttributes} class={classes}>{@render children()}</button>
{:else}
	<a {...rest as HTMLAnchorAttributes} class={classes}>{@render children()}</a>
{/if}
