<script lang="ts">
	import { Search } from '@lucide/svelte';
	import { page } from '$app/state';
	import ArtistResults from '#lib/components/supports/ArtistResults.svelte';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>Artists · Local Network</title>
	<meta name="description" content="Find out which Local Network DJs played an artist's tracks, and where." />
</svelte:head>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Artists" tone="high" title="Who played my track?" align="split" lede="Find an artist to see which of our DJs played their tracks, when and where.">
		{#snippet actions()}<Button variant="secondary" href="/charts">See the charts</Button>{/snippet}
	</SectionHeader>

	<form method="GET" role="search" aria-label="Find an artist" class="flex gap-2" data-sveltekit-reset="false">
		<label class="sr-only" for="q">Artist</label>
		<input id="q" name="q" class="input" value={data.q} placeholder="Artist name" maxlength="100" />
		<button class="btn-primary"><Search class="size-4" />Search</button>
	</form>

	<ArtistResults artists={data.artists} q={data.q} retryHref={page.url.pathname + page.url.search} />
</div>
