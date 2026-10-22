<script module lang="ts">
	import type { WithElementRef } from "bits-ui";
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from "svelte/elements";
	import { cn } from "$lib/utils.ts";

	// This was `tv()` from tailwind-variants, which for what it is asked to do
	// here — one base, two flat variant groups, no slots, no compound rules —
	// is 89 KB of bundle for a lookup and a join. `cn` still does the merging
	// that matters (a caller's `size-7` beating the base's `size-8`), so the
	// classes a button ends up with are the same ones.
	const VARIANTS = {
		// The website's buttons (btn-primary, btn-secondary, btn-ghost, btn-danger in
		// packages/tokens/theme.css), a size down for a 400px panel. Focus draws the
		// theme's own outline.
		base: "rounded-md border border-transparent text-label font-semibold aria-invalid:border-critical [&_svg:not([class*='size-'])]:size-4 group/button inline-flex shrink-0 items-center justify-center whitespace-nowrap transition-colors select-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0",
		variant: {
			default: "bg-fg text-fg-inverse hover:bg-fg-soft",
			outline: "border-line-strong bg-raised text-fg hover:border-fg-subtle aria-expanded:border-fg-subtle",
			secondary: "bg-well text-fg hover:bg-line aria-expanded:bg-line",
			ghost: "text-fg-muted hover:bg-raised hover:text-fg aria-expanded:bg-raised aria-expanded:text-fg",
			destructive: "border-critical/40 text-critical hover:bg-critical/10",
			link: "text-link underline decoration-link/40 underline-offset-4 hover:decoration-link",
		},
		size: {
			default: "h-8 gap-1.5 px-3 has-data-[icon=inline-end]:pr-2.5 has-data-[icon=inline-start]:pl-2.5",
			xs: "h-6 gap-1 rounded-sm px-2 text-caption has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
			sm: "h-7 gap-1 rounded-sm px-2.5 text-caption has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2 [&_svg:not([class*='size-'])]:size-3.5",
			lg: "h-9 gap-1.5 px-3.5 text-small has-data-[icon=inline-end]:pr-3 has-data-[icon=inline-start]:pl-3",
			icon: "size-8",
			"icon-xs": "size-6 rounded-sm [&_svg:not([class*='size-'])]:size-3",
			"icon-sm": "size-7 rounded-sm",
			"icon-lg": "size-9",
		},
	};


	export type ButtonVariant = keyof typeof VARIANTS.variant;

	export type ButtonSize = keyof typeof VARIANTS.size;

	export type ButtonProps = WithElementRef<HTMLButtonAttributes> &
		WithElementRef<HTMLAnchorAttributes> & {
			variant?: ButtonVariant;
			size?: ButtonSize;
		};

	/**
	 * The classes for one variant and size, in the call shape `tv()` gave —
	 * which is the shape shadcn-svelte re-exports this under, for anything
	 * that wants a button's look without being one.
	 */
	export const buttonVariants = ({
		variant = "default",
		size = "default",
	}: { variant?: ButtonVariant; size?: ButtonSize } = {}) =>
		`${VARIANTS.base} ${VARIANTS.variant[variant] ?? VARIANTS.variant.default} ${
			VARIANTS.size[size] ?? VARIANTS.size.default
		}`;
</script>

<script lang="ts">
	let {
		class: className,
		variant = "default",
		size = "default",
		ref = $bindable(null),
		href = undefined,
		type = "button",
		disabled,
		children,
		...restProps
	}: ButtonProps = $props();
</script>

{#if href}
	<a
		bind:this={ref}
		data-slot="button"
		class={cn(buttonVariants({ variant, size }), className)}
		href={disabled ? undefined : href}
		aria-disabled={disabled}
		role={disabled ? "link" : undefined}
		tabindex={disabled ? -1 : undefined}
		{...restProps}
	>
		{@render children?.()}
	</a>
{:else}
	<button
		bind:this={ref}
		data-slot="button"
		class={cn(buttonVariants({ variant, size }), className)}
		{type}
		{disabled}
		{...restProps}
	>
		{@render children?.()}
	</button>
{/if}