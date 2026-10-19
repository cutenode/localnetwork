<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import { pluralize } from '#lib/format.ts';
	import { artistHref, musicBrainzUrl } from '#lib/supports.ts';
	import type { SupportedArtist } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import GhostRows from './GhostRows.svelte';

	/**
	 * Artists matching a search `q`, or the most supported ones without one, as they stream in. `artists` is null when
	 * the directory is offline; `retryHref` asks again.
	 */
	let { artists, q, retryHref }: { artists: Promise<SupportedArtist[] | null> | SupportedArtist[] | null; q: string; retryHref: string } = $props();
</script>

<section aria-labelledby="artists-title" class="flex min-w-0 flex-col gap-3">
	<h2 id="artists-title" class="m-0 text-heading font-semibold">{q ? `Artists matching “${q}”` : 'Most supported'}</h2>
	{#await artists}
		<GhostRows label="Loading artists" />
	{:then result}
		{#if !result}
			<EmptyState title="Artists offline" body="The artist search isn't answering. Try again in a minute."><Button variant="secondary" href={retryHref}>Try again</Button></EmptyState>
		{:else if result.length === 0}
			{#if q}
				<EmptyState title="No artists match" body="Only artists our DJs have played show up. Check the spelling, or try part of the name." />
			{:else}
				<EmptyState title="No plays yet" body="Artists show up here once our DJs publish their sets." />
			{/if}
		{:else}
			<ul class="card m-0 flex list-none flex-col divide-y divide-line p-0">
				{#each result as artist (artist.mbid ?? artist.name)}
					<li class="flex items-baseline gap-3 px-4 py-3">
						<span class="flex min-w-0 flex-1 items-baseline gap-2">
							<a class="min-w-0 font-semibold text-fg no-underline hover:underline" href={artistHref(artist)}>{artist.name}</a>
							{#if artist.mbid}<a class="inline-flex self-center text-fg-subtle hover:text-fg" href={musicBrainzUrl('artist', artist.mbid)} rel="noopener noreferrer" target="_blank" aria-label="{artist.name} on MusicBrainz"><ExternalLink class="size-3.5" /></a>{/if}
						</span>
						<span class="font-mono text-label whitespace-nowrap text-fg-soft">{pluralize(artist.supports, 'all-time support')} · {pluralize(artist.djCount, 'DJ')}</span>
					</li>
				{/each}
			</ul>
		{/if}
	{:catch}
		<EmptyState title="Couldn't search artists" body="Something went wrong on our side. Try again in a minute."><Button variant="secondary" href={retryHref}>Try again</Button></EmptyState>
	{/await}
</section>
