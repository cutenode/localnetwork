<script lang="ts">
	import { Lock } from '@lucide/svelte';
	import type { EpkMix } from '#lib/epk.ts';

	/**
	 * A press kit's mixes as a list that prints: each links out, with its address written under it on
	 * paper. Mixes only people with access hear are marked; ones with a tracklist link to it.
	 */
	let {
		mixes,
		tracklistHref
	}: { mixes: (EpkMix & { onRequest?: boolean })[]; tracklistHref?: (url: string) => string | null } = $props();
</script>

<ol class="m-0 flex list-none flex-col p-0">
	{#each mixes as mix (mix.url)}
		{@const tracklist = tracklistHref?.(mix.url)}
		<li class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-t border-line py-3 last:border-b">
			<span class="inline-flex items-baseline gap-2">
				<a class="font-semibold text-fg no-underline hover:underline" href={mix.url} rel="noopener noreferrer nofollow" target="_blank">{mix.title}</a>
				{#if mix.onRequest}<span class="inline-flex items-center gap-1 text-caption text-fg-subtle"><Lock class="size-3" aria-hidden="true" />On request</span>{/if}
			</span>
			<span class="flex flex-wrap gap-x-4 text-small text-fg-muted">
				{#if mix.durationMinutes}<span class="font-mono text-label">{mix.durationMinutes} min</span>{/if}
				{#if mix.year}<span class="font-mono text-label">{mix.year}</span>{/if}
				{#if tracklist}<a class="link print:hidden" href={tracklist}>Tracklist</a>{/if}
				<span class="hidden break-all print:inline">{mix.url}</span>
			</span>
		</li>
	{/each}
</ol>
