<script lang="ts">
	import { Download, Lock } from '@lucide/svelte';
	import type { EpkPhoto } from '#lib/epk.ts';

	/** Press photos to look at and download, each with its photographer's credit. */
	let { photos, base }: { photos: (EpkPhoto & { onRequest?: boolean })[]; base: string } = $props();
</script>

<ul class="m-0 grid list-none grid-cols-2 gap-3 p-0 sm:grid-cols-3">
	{#each photos as photo (photo.image.ref.$link)}
		{@const src = `${base}/${photo.image.ref.$link}`}
		<li class="flex flex-col gap-1.5 print:break-inside-avoid">
			<a href={src} target="_blank" rel="noopener" class="block overflow-hidden rounded-lg border border-line bg-raised">
				<img {src} alt={photo.alt} loading="lazy" class="aspect-[4/5] w-full object-cover" />
			</a>
			<p class="m-0 flex items-center justify-between gap-2 text-caption text-fg-muted">
				<span class="inline-flex min-w-0 items-center gap-1 truncate">
					{#if photo.onRequest}<Lock class="size-3 shrink-0" aria-label="On request" />{/if}
					{photo.credit ? `Photo: ${photo.credit}` : (photo.aspectRatio ? `${photo.aspectRatio.width} × ${photo.aspectRatio.height}` : '')}
				</span>
				<a href="{src}?download" class="inline-flex shrink-0 items-center gap-1 text-fg-muted print:hidden" aria-label="Download photo{photo.credit ? ` by ${photo.credit}` : ''}"><Download class="size-3.5" aria-hidden="true" />Download</a>
			</p>
		</li>
	{/each}
</ul>
