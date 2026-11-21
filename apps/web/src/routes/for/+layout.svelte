<script lang="ts">
	import { onNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import ForCrumbs from '#lib/components/landing/ForCrumbs.svelte';
	import { sides } from '#lib/components/landing/SideMark.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	// The side whose page this is, or none on /for itself.
	const side = $derived(page.route.id === sides.djs.href ? 'djs' : page.route.id === sides.promoters.href ? 'promoters' : undefined);

	// Between /for and its two pages the browser morphs the old page into the new one, so a door's label grows into the
	// crumb and the crumb and the switch to the other side trade places. Leaving /for, and with reduced motion, it's an
	// ordinary navigation.
	onNavigate((navigation) => {
		const staysInFor = navigation.to?.route.id?.startsWith('/for') ?? false;

		if (!staysInFor || !document.startViewTransition || matchMedia('(prefers-reduced-motion: reduce)').matches) return;

		return new Promise((resolve) => {
			document.startViewTransition(async () => {
				resolve();
				await navigation.complete;
			});
		});
	});
</script>

{#if side}<ForCrumbs {side} />{/if}
<!-- Named, so the page under the bar fades on its own while the header and bar, unchanged, stay put. -->
<div style:view-transition-name="for-page">{@render children()}</div>

<style>
	/* The sides slide to their new places. The page and the sides' labels fade out before the new ones fade in, so two
	   headlines or two labels never sit on top of each other. */
	:global(::view-transition-group(side-djs)),
	:global(::view-transition-group(side-promoters)) {
		animation-duration: 450ms;
		animation-timing-function: cubic-bezier(0.22, 1, 0.36, 1);
	}
	:global(::view-transition-old(for-page)),
	:global(::view-transition-old(side-djs)),
	:global(::view-transition-old(side-promoters)) {
		animation-duration: 150ms;
	}
	:global(::view-transition-new(for-page)),
	:global(::view-transition-new(side-djs)),
	:global(::view-transition-new(side-promoters)) {
		animation-duration: 260ms;
		animation-delay: 130ms;
	}
</style>
