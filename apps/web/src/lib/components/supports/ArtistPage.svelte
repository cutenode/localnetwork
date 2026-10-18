<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { page } from '$app/state';
	import { artistHref } from '#lib/supports.ts';
	import type { ArtistSupports } from '#lib/types.ts';
	import ArtistSupport from './ArtistSupport.svelte';

	/**
	 * An artist's page, by MBID or by `name`. The head is right before the data comes: the name page knows its name,
	 * and the MBID page its canonical address; a name page learns its canonical address (the MBID page, when sets give
	 * the name one) from the data. `origin` is the site's public origin.
	 */
	let {
		name,
		canonical,
		origin,
		supports
	}: { name: string | null; canonical: string | null; origin: string; supports: Promise<ArtistSupports | null> } = $props();

	const about = $derived(name ? `${name}'s` : "this artist's");
</script>

<svelte:head>
	{#await supports}
		<title>{name ?? 'Artist'} · Local Network</title>
	{:then result}
		<title>{result?.artist?.name ?? name ?? 'Artist'} · Local Network</title>
		{#if !canonical && result?.artist}<link rel="canonical" href={origin + artistHref(result.artist)} />{/if}
	{:catch}
		<title>{name ?? 'Artist'} · Local Network</title>
	{/await}
	<meta name="description" content="Which Local Network DJs played {about} tracks, and where." />
	{#if canonical}<link rel="canonical" href={origin + canonical} />{/if}
</svelte:head>

<div class="container-page flex flex-col gap-8 py-10 sm:py-14">
	<a href="/artists" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Artists</a>
	<ArtistSupport {supports} retryHref={page.url.pathname + page.url.search} />
</div>
