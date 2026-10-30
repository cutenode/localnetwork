<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import { type DjSet, setPlace, trackBpm } from '#lib/dj.ts';
	import { clock } from '#lib/mix/report.ts';
	import { musicBrainzUrl, trackArtists } from '#lib/supports.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import ArtistLinks from './supports/ArtistLinks.svelte';
	import { webHref } from '#lib/web-url.ts';

	/** Sets a DJ played, newest first, each with its tracklist folded away under it. */
	let { sets, limit = 5 }: { sets: DjSet[]; limit?: number } = $props();

	const shown = $derived(sets.slice(0, limit));
</script>

<ol class="m-0 flex list-none flex-col gap-2.5 p-0">
	{#each shown as set (set.uri)}
		<li class="card flex flex-col gap-3 p-4">
			<div class="flex items-start gap-4">
				<DateBlock date={set.date} />
				<div class="flex min-w-0 flex-1 flex-col gap-1">
					<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
						<span class="text-emphasis font-semibold">{set.title ?? setPlace(set) ?? 'Untitled set'}</span>
						{#if set.kind}<Tag variant="mono">{set.kind}</Tag>{/if}
					</p>
					<p class="m-0 text-small text-fg-muted">
						{[setPlace(set), set.playedWith?.length && `with ${set.playedWith.map((partner) => partner.name).join(', ')}`, `${set.tracks.length} ${set.tracks.length === 1 ? 'track' : 'tracks'}`].filter(Boolean).join(' · ')}
					</p>
					{#if set.links?.length}
						<p class="m-0 flex flex-wrap gap-x-4 text-small">
							{#each set.links as link (link.uri)}<a class="link inline-flex items-center gap-1" href={webHref(link.uri)} rel="noopener noreferrer nofollow" target="_blank">{link.name ?? 'Listen'}<ExternalLink class="size-3" /></a>{/each}
						</p>
					{/if}
				</div>
			</div>
			{#if set.tracks.length}
				<details class="group border-t border-line pt-2 print:hidden">
					<summary class="cursor-pointer text-small text-fg-muted marker:text-fg-subtle hover:text-fg">Tracklist</summary>
					<ol class="m-0 mt-2 list-none p-0">
						{#each set.tracks as track, i (i)}
							{@const artists = track.unidentified ? [] : trackArtists(track)}
							<li class="grid grid-cols-[3rem_minmax(0,1fr)_auto_2.5rem] items-baseline gap-3 py-1.5 text-small">
								<span class="font-mono text-caption text-fg-subtle">{track.startSeconds === undefined ? i + 1 : clock(track.startSeconds)}</span>
								<span class="min-w-0"
									><span class="font-semibold"
										>{track.unidentified ? 'Unreleased' : (track.title ?? 'Untitled')}{#if track.recordingMbId && !track.unidentified}{' '}<a class="inline-flex text-fg-subtle hover:text-fg" href={musicBrainzUrl('recording', track.recordingMbId)} rel="noopener noreferrer" target="_blank" aria-label="{track.title ?? 'Track'} on MusicBrainz"><ExternalLink class="size-3" /></a>{/if}</span
									>{#if artists.length}{' '}<ArtistLinks {artists} />{/if}</span
								>
								<span class="font-mono text-caption text-fg-muted max-sm:hidden">{trackBpm(track.bpmCentis) ?? ''}</span>
								<span class="text-center font-mono text-caption">{track.key ?? ''}</span>
							</li>
						{/each}
					</ol>
				</details>
			{/if}
		</li>
	{/each}
</ol>
