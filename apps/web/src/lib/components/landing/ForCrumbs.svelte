<script lang="ts">
	import { ArrowRight, ChevronRight } from '@lucide/svelte';
	import SideMark, { type Side, sides } from './SideMark.svelte';

	/**
	 * The bar under the header on /for/djs and /for/promoters: For › this side, in its colour with its mark, and the
	 * other side a click away. Always one row of the same height, so it never moves the page under it.
	 */
	let { side }: { side: Side } = $props();

	const current = $derived(sides[side]);

	const other = $derived(sides[side === 'djs' ? 'promoters' : 'djs']);
</script>

<div class="border-b border-line bg-canvas">
	<div class="container-page flex h-13 items-center justify-between gap-4">
		<nav aria-label="Breadcrumb">
			<ol class="flex items-center gap-1.5 text-small">
				<li><a href="/for" class="rounded-md px-1.5 py-1 text-fg-muted no-underline transition-colors hover:text-fg">For</a></li>
				<li class="flex items-center gap-1.5">
					<ChevronRight aria-hidden="true" class="size-4 text-fg-subtle" />
					<a
						href={current.href}
						aria-current="page"
						class={['pill group inline-flex h-8 items-center gap-2 rounded-full border px-3 font-semibold no-underline transition-colors', current.tone]}
						style:view-transition-name={current.transition}
					>
						<SideMark {side} />
						<span class="text-fg">{current.label}</span>
					</a>
				</li>
			</ol>
		</nav>
		<a href={other.href} class="group inline-flex h-8 items-center gap-2 rounded-full px-3 text-small text-fg-muted no-underline transition-colors hover:bg-well hover:text-fg" style:view-transition-name={other.transition}>
			<SideMark side={side === 'djs' ? 'promoters' : 'djs'} />
			<span>{other.title}</span>
			<ArrowRight aria-hidden="true" class="size-4 transition-transform duration-300 ease-out group-hover:translate-x-1 motion-reduce:transition-none" />
		</a>
	</div>
</div>

<style>
	/* Tinted with the side's own colour, which the tone class sets as currentColor. */
	.pill {
		border-color: color-mix(in oklab, currentColor 40%, transparent);
		background: color-mix(in oklab, currentColor 10%, transparent);
	}
	.pill:hover {
		background: color-mix(in oklab, currentColor 20%, transparent);
	}
</style>
