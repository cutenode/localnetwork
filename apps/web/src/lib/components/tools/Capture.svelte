<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import type { Capture } from '#lib/tools/types.ts';
	import { slimCapture } from './sources.ts';

	/**
	 * A capture of a tool at work: AVIF or WebP at the width it's shown at, and on a phone the capture
	 * taken at a phone's width. A browser with neither format gets the `<img>`'s one fallback file.
	 * Every source carries its size, so its box is set before it loads. `sizes` is how wide it's
	 * shown, for the captures imported at several widths. `eager` is for the one at the top of a page,
	 * which is likely its largest paint. `class` replaces the image's own sizing and corners.
	 */
	let { capture, sizes, eager = false, onsettle, class: extra = 'h-auto w-full rounded-xl' }: { capture: Capture; sizes?: string; eager?: boolean; onsettle?: () => void; class?: ClassValue } = $props();

	const { picture, phone } = $derived(slimCapture(capture));

	// Listened for in the browser, not set as `onload`: on a server-rendered image Svelte writes that
	// out as an inline handler, which the site's CSP blocks. An image that settled first counts too,
	// and one that fails to load is as done as one that loads: its alt text stands in for it.
	function settled(img: HTMLImageElement) {
		if (!onsettle) return;

		if (img.complete) {
			onsettle();

			return;
		}

		img.addEventListener('load', onsettle, { once: true });
		img.addEventListener('error', onsettle, { once: true });

		return () => {
			img.removeEventListener('load', onsettle);
			img.removeEventListener('error', onsettle);
		};
	}
</script>

<picture>
	{#if phone}
		{#each Object.entries(phone.sources) as [format, srcset] (format)}
			<source media="(width < 40rem)" type="image/{format}" {srcset} width={phone.img.w} height={phone.img.h} />
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
		{@attach settled}
		class={['block', extra]}
	/>
</picture>
