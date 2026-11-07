<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import { setPlace } from '#lib/dj.ts';
	import { accountLabel, pluralize } from '#lib/format.ts';
	import { clock } from '#lib/mix/report.ts';
	import { musicBrainzUrl } from '#lib/supports.ts';
	import type { ArtistSupports, TrackPlay } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import ArtistLinks from './ArtistLinks.svelte';

	/**
	 * An artist's page below the breadcrumb: their name and supports, then each of their tracks DJs played with every
	 * play. `supports` is null when the directory is offline; `retryHref` asks again.
	 */
	let { supports, retryHref }: { supports: ArtistSupports | null; retryHref: string } = $props();

	/** Plays shown before the rest fold away. */
	const SHOWN = 8;
</script>

{#snippet play(item: TrackPlay, djs: ArtistSupports['djs'])}
	{@const dj = djs[item.did]}
	<li class="flex items-start gap-4 px-4 py-3">
		<DateBlock date={item.set.date} />
		<div class="flex min-w-0 flex-1 flex-col gap-1">
			<p class="m-0">
				{#each dj?.projects ?? [] as project, i (project.slug)}{#if i > 0},{' '}{/if}<a class="font-semibold text-fg no-underline hover:underline" href="/projects/{project.slug}">{project.name}</a>{/each}
				<span class="text-small text-fg-muted">{accountLabel(dj ?? { did: item.did, handle: null })}</span>
			</p>
			<p class="m-0 flex flex-wrap items-baseline gap-x-2 gap-y-1 text-small text-fg-muted">
				{setPlace(item.set) ?? item.set.title ?? 'Somewhere'}
				{#if item.set.kind}<Tag variant="mono">{item.set.kind}</Tag>{/if}
				{#if item.startSeconds !== undefined}<span class="font-mono text-label text-fg-subtle">{clock(item.startSeconds)} in<span class="sr-only">{' '}the set</span></span>{/if}
			</p>
		</div>
	</li>
{/snippet}

{#if !supports}
	<h1 class="m-0 text-title font-semibold">Artist</h1>
	<EmptyState title="Artists offline" body="Who played what isn't answering. Try again in a minute."><Button variant="secondary" href={retryHref}>Try again</Button></EmptyState>
{:else if !supports.artist}
	<h1 class="m-0 text-title font-semibold">Artist</h1>
	<EmptyState title="No plays yet" body="None of our DJs' sets credit this artist yet."><Button variant="secondary" href="/artists">Find another artist</Button></EmptyState>
{:else}
	{@const artist = supports.artist}
	<header class="flex flex-col gap-3">
		<h1 class="m-0 text-title font-semibold text-balance">{artist.name}</h1>
		<p class="m-0 flex flex-wrap items-center gap-x-4 gap-y-1 text-small text-fg-muted">
			<span><span class="font-mono text-fg">{artist.supports.toLocaleString('en')}</span> all-time {artist.supports === 1 ? 'support' : 'supports'} from <span class="font-mono text-fg">{artist.djCount.toLocaleString('en')}</span> {artist.djCount === 1 ? 'DJ' : 'DJs'}</span>
			{#if artist.mbid}<a class="link inline-flex items-center gap-1" href={musicBrainzUrl('artist', artist.mbid)} rel="noopener noreferrer" target="_blank">MusicBrainz<ExternalLink class="size-3" /></a>{/if}
		</p>
	</header>
	<section aria-labelledby="tracks-title" class="flex flex-col gap-3">
		<h2 id="tracks-title" class="m-0 text-heading font-semibold">Tracks</h2>
		<ol class="m-0 flex list-none flex-col gap-4 p-0">
			{#each supports.tracks as { track, plays } (track.id)}
				<li class="card overflow-hidden">
					<div class="flex items-baseline justify-between gap-3 border-b border-line px-4 py-3">
						<div class="flex min-w-0 flex-col">
							<h3 class="m-0 text-emphasis font-semibold">
								{track.title}{#if track.recordingMbId}{' '}<a class="inline-flex text-fg-subtle hover:text-fg" href={musicBrainzUrl('recording', track.recordingMbId)} rel="noopener noreferrer" target="_blank" aria-label="{track.title} on MusicBrainz"><ExternalLink class="size-3.5" /></a>{/if}
							</h3>
							<span class="text-small"><ArtistLinks artists={track.artists} /></span>
						</div>
						<span class="font-mono text-label whitespace-nowrap text-fg-soft">{pluralize(track.supports, 'DJ')}</span>
					</div>
					<ol aria-label="Plays of {track.title}" class="m-0 flex list-none flex-col divide-y divide-line p-0">
						{#each plays.slice(0, SHOWN) as item, i (i)}{@render play(item, supports.djs)}{/each}
					</ol>
					{#if plays.length > SHOWN}
						<details class="border-t border-line">
							<summary class="cursor-pointer px-4 py-2.5 text-small text-fg-muted marker:text-fg-subtle hover:text-fg">{pluralize(plays.length - SHOWN, 'more play')}</summary>
							<ol aria-label="More plays of {track.title}" class="m-0 flex list-none flex-col divide-y divide-line border-t border-line p-0">
								{#each plays.slice(SHOWN) as item, i (i)}{@render play(item, supports.djs)}{/each}
							</ol>
						</details>
					{/if}
				</li>
			{/each}
		</ol>
	</section>
{/if}
