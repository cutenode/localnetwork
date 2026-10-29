<script lang="ts">
	import { ExternalLink, Pause, Play } from '@lucide/svelte';
	import { type DjSet, setPlace, trackBpm } from '#lib/dj.ts';
	import { clock } from '#lib/mix/report.ts';
	import { type PlayerSource, type PlayerTrack, playable } from '#lib/mix/source.ts';
	import { musicBrainzUrl, trackArtists } from '#lib/supports.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import SetPlayer from './mix/SetPlayer.svelte';
	import ArtistLinks from './supports/ArtistLinks.svelte';

	/**
	 * Sets a DJ played, newest first, each with its tracklist folded away under it. A set with a recording plays
	 * here, its tracklist marking the track playing and starting any track it's given a time for.
	 */
	let { sets, limit = 5 }: { sets: DjSet[]; limit?: number } = $props();

	const shown = $derived(sets.slice(0, limit));

	/** The set whose player is open, by URI: one at a time. */
	let open = $state<string | null>(null);

	let player = $state<SetPlayer>();

	let current = $state(-1);

	/** Asks the server where a set's recording plays, when someone presses Play. */
	const load = (uri: string) => async () => {
		const [did, , rkey] = uri.slice('at://'.length).split('/');
		const response = await fetch(`/sets/${did}/${rkey}/player.json`);

		if (!response.ok) throw new Error(`player.json: HTTP ${response.status}`);

		// SAFETY: player.json answers `{ source }`, a PlayerSource the server built (routes/sets/[did]/[rkey]/player.json).
		return ((await response.json()) as { source: PlayerSource | null }).source;
	};

	const name = (set: DjSet) => set.title ?? setPlace(set) ?? 'Untitled set';

	const trackTitle = (track: DjSet['tracks'][number]) => (track.unidentified ? 'Unreleased' : (track.title ?? 'Untitled'));

	/** A set's tracks as the player names them, artist and all, for the system's media controls. */
	const playerTracks = (set: DjSet): PlayerTrack[] =>
		set.tracks.map((track) => ({
			title: trackTitle(track),
			artist: track.unidentified ? undefined : (track.artist ?? (trackArtists(track).map((artist) => artist.name).join(', ') || undefined)),
			startSeconds: track.startSeconds
		}));
</script>

<ol class="m-0 flex list-none flex-col gap-2.5 p-0">
	{#each shown as set, n (set.uri)}
		{@const isOpen = open === set.uri}
		<li class="card flex flex-col gap-3 p-4">
			<div class="flex items-start gap-4">
				<DateBlock date={set.date} />
				<div class="flex min-w-0 flex-1 flex-col gap-1">
					<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
						<span class="text-emphasis font-semibold">{name(set)}</span>
						{#if set.kind}<Tag variant="mono">{set.kind}</Tag>{/if}
					</p>
					<p class="m-0 text-small text-fg-muted">
						{[setPlace(set), set.playedWith?.length && `with ${set.playedWith.map((partner) => partner.name).join(', ')}`, `${set.tracks.length} ${set.tracks.length === 1 ? 'track' : 'tracks'}`].filter(Boolean).join(' · ')}
					</p>
					{#if set.links?.length}
						<p class="m-0 flex flex-wrap gap-x-4 text-small">
							{#each set.links as link (link.uri)}<a class="link inline-flex items-center gap-1" href={link.uri} rel="noopener noreferrer nofollow" target="_blank">{link.name ?? 'Listen'}<ExternalLink class="size-3" /></a>{/each}
						</p>
					{/if}
				</div>
				{#if playable(set)}
					<button
						type="button"
						aria-label={isOpen ? `Close the player for ${name(set)}` : `Play ${name(set)}`}
						aria-expanded={isOpen}
						aria-controls="set-player-{n}"
						class="grid size-10 shrink-0 cursor-pointer place-items-center rounded-full bg-fg text-fg-inverse transition-opacity hover:opacity-85 print:hidden"
						onclick={() => {
							open = isOpen ? null : set.uri;
							current = -1;
						}}
					>
						{#if isOpen}<Pause class="size-4 fill-current" />{:else}<Play class="ml-0.5 size-4 fill-current" />{/if}
					</button>
				{/if}
			</div>
			{#if isOpen}
				<SetPlayer bind:this={player} bind:current id="set-player-{n}" load={load(set.uri)} title={name(set)} tracks={playerTracks(set)} />
			{/if}
			{#if set.tracks.length}
				<!-- Open while the player is, so the tracklist can follow along. -->
				<details class="group border-t border-line pt-2 print:hidden" open={isOpen || undefined}>
					<summary class="cursor-pointer text-small text-fg-muted marker:text-fg-subtle hover:text-fg">Tracklist</summary>
					<ol class="m-0 mt-2 list-none p-0">
						{#each set.tracks as track, i (i)}
							{@const artists = track.unidentified ? [] : trackArtists(track)}
							{@const isCurrent = isOpen && current === i}
							{@const start = isOpen ? track.startSeconds : undefined}
							<!-- The start time is the row's button, for keyboards and screen readers; a click anywhere else on the row (but its links) does the same. -->
							<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
							<li
								aria-current={isCurrent || undefined}
								class={['-mx-1.5 grid grid-cols-[3rem_minmax(0,1fr)_auto_2.5rem] items-baseline gap-3 rounded-md px-1.5 py-1.5 text-small', start !== undefined && 'cursor-pointer hover:bg-raised', isCurrent && 'bg-raised shadow-[inset_3px_0_0_var(--color-fg)]']}
								onclick={(event) => {
									if (start !== undefined && !(event.target as Element).closest('a, button')) player?.seek(start);
								}}
							>
								{#if start === undefined}
									<span class="font-mono text-caption text-fg-subtle">{track.startSeconds === undefined ? i + 1 : clock(track.startSeconds)}</span>
								{:else}
									<button type="button" class="cursor-pointer text-left font-mono text-caption text-fg-subtle underline-offset-2 hover:text-fg hover:underline" aria-label="Play from {clock(start)}: {trackTitle(track)}" onclick={() => player?.seek(start)}>{clock(start)}</button>
								{/if}
								<span class="min-w-0"
									><span class="font-semibold"
										>{trackTitle(track)}{#if track.recordingMbId && !track.unidentified}{' '}<a class="inline-flex text-fg-subtle hover:text-fg" href={musicBrainzUrl('recording', track.recordingMbId)} rel="noopener noreferrer" target="_blank" aria-label="{track.title ?? 'Track'} on MusicBrainz"><ExternalLink class="size-3" /></a>{/if}</span
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
