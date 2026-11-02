<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import type { Capture } from '#lib/tools/types.ts';

	/**
	 * A capture of a tool at work: AVIF or WebP at the width it's shown at, and on a phone the capture
	 * taken at a phone's width. Every source carries its size, so its box is set before it loads.
	 * `sizes` is how wide it's shown, for the captures imported at several widths. `eager` is for the
	 * one at the top of a page, which is likely its largest paint.
	 */
	let { capture, sizes, eager = false, onload, class: extra }: { capture: Capture; sizes?: string; eager?: boolean; onload?: () => void; class?: ClassValue } = $props();

	const { picture, phone } = $derived(capture);

	// Listened for in the browser, not set as `onload`: on a server-rendered image Svelte writes that
	// out as an inline handler, which the site's CSP blocks. An image that loaded first counts too.
	function loaded(img: HTMLImageElement) {
		if (!onload) return;

		if (img.complete && img.naturalWidth) {
			onload();

			return;
		}

		img.addEventListener('load', onload, { once: true });

		return () => img.removeEventListener('load', onload);
	}
</script>

<!-- The page's largest paint, asked for with the page rather than once the browser reaches it. -->
<svelte:head>
	{#if eager}
		{#if phone}<link rel="preload" as="image" type="image/avif" media="(max-width: 639px)" imagesrcset={phone.sources.avif} fetchpriority="high" />{/if}
		<link rel="preload" as="image" type="image/avif" media={phone ? '(min-width: 640px)' : undefined} imagesrcset={picture.sources.avif} imagesizes={sizes} fetchpriority="high" />
	{/if}
</svelte:head>

<picture>
	{#if phone}
		{#each Object.entries(phone.sources) as [format, srcset] (format)}
			<source media="(max-width: 639px)" type="image/{format}" {srcset} width={phone.img.w} height={phone.img.h} />
		{/each}
	{/if}
	{#each Object.entries(picture.sources) as [format, srcset] (format)}
		<source type="image/{format}" {srcset} {sizes} width={picture.img.w} height={picture.img.h} />
	{/each}
	<img
		src={picture.img.src}
		alt={capture.alt}
		width={picture.img.w}
		height={picture.img.h}
		loading={eager ? 'eager' : 'lazy'}
		fetchpriority={eager ? 'high' : undefined}
		decoding="async"
		{@attach loaded}
		class={['block h-auto w-full rounded-xl', extra]}
	/>
</picture>
