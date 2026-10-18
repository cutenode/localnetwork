<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import { pluralize } from '#lib/format.ts';
	import { artistHref, musicBrainzUrl } from '#lib/supports.ts';
	import type { Charts } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import ArtistLinks from './ArtistLinks.svelte';
	import GhostRows from './GhostRows.svelte';

	/**
	 * The charts' two ranked lists, tracks and then artists (beside each other on a wide screen), as they stream in.
	 * `charts` is null when the directory is offline; `retryHref` reloads the same charts.
	 */
	let { charts, retryHref }: { charts: Promise<Charts | null> | Charts | null; retryHref: string } = $props();
</script>

{#snippet offline(title: string, body: string)}
	<div class="xl:col-span-2">
		<EmptyState {title} {body}><Button variant="secondary" href={retryHref}>Try again</Button></EmptyState>
	</div>
{/snippet}

{#snippet list(id: string, title: string, rows: Snippet)}
	<section aria-labelledby={id} class="flex min-w-0 flex-col gap-3">
		<h2 {id} class="m-0 text-heading font-semibold">{title}</h2>
		{@render rows()}
	</section>
{/snippet}

<div class="grid items-start gap-x-8 gap-y-10 xl:grid-cols-2">
	{#await charts}
		{#snippet tracksLoading()}<GhostRows label="Loading tracks" />{/snippet}
		{#snippet artistsLoading()}<GhostRows label="Loading artists" />{/snippet}
		{@render list('chart-tracks', 'Tracks', tracksLoading)}
		{@render list('chart-artists', 'Artists', artistsLoading)}
	{:then result}
		{#if !result}
			{@render offline('Charts offline', "The charts aren't answering. Try again in a minute.")}
		{:else if result.tracks.length === 0}
			<div class="xl:col-span-2">
				<EmptyState title="Nothing charted yet" body="A track charts once three DJs play it. Try another month or every city." />
			</div>
		{:else}
			{#snippet tracks()}
				<ol class="card m-0 flex list-none flex-col divide-y divide-line p-0">
					{#each result.tracks as track, i (track.id)}
						<li class="grid grid-cols-[1.5rem_minmax(0,1fr)_auto] items-baseline gap-3 px-4 py-3">
							<span class="font-mono text-label text-fg-subtle">{i + 1}</span>
							<span class="flex min-w-0 flex-col">
								<span class="font-semibold"
									>{track.title}{#if track.recordingMbId}{' '}<a class="inline-flex text-fg-subtle hover:text-fg" href={musicBrainzUrl('recording', track.recordingMbId)} rel="noopener noreferrer" target="_blank" aria-label="{track.title} on MusicBrainz"><ExternalLink class="size-3.5" /></a>{/if}</span
								>
								<span class="text-small"><ArtistLinks artists={track.artists} /></span>
							</span>
							<span class="font-mono text-label whitespace-nowrap text-fg-soft">{pluralize(track.supports, 'DJ')}</span>
						</li>
					{/each}
				</ol>
			{/snippet}
			{#snippet artists()}
				<ol class="card m-0 flex list-none flex-col divide-y divide-line p-0">
					{#each result.artists as artist, i (artist.mbid ?? artist.name)}
						<li class="grid grid-cols-[1.5rem_minmax(0,1fr)_auto] items-baseline gap-3 px-4 py-3">
							<span class="font-mono text-label text-fg-subtle">{i + 1}</span>
							<span class="flex min-w-0 flex-col">
								<a class="font-semibold text-fg no-underline hover:underline" href={artistHref(artist)}>{artist.name}</a>
								<span class="text-small text-fg-muted">from {pluralize(artist.djCount, 'DJ')}</span>
							</span>
							<span class="font-mono text-label whitespace-nowrap text-fg-soft">{pluralize(artist.supports, 'support')}</span>
						</li>
					{/each}
				</ol>
			{/snippet}
			{@render list('chart-tracks', 'Tracks', tracks)}
			{@render list('chart-artists', 'Artists', artists)}
		{/if}
	{:catch}
		{@render offline("Couldn't load the charts", 'Something went wrong on our side. Try again in a minute.')}
	{/await}
</div>
