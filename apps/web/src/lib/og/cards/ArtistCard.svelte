<script module lang="ts">
	/** What an artist's card shows: how many DJs played them, and their most played tracks with each one's supports. */
	export type ArtistCardProps = { name: string; djCount: number; tracks: { title: string; supports: number }[] };
</script>

<script lang="ts">
	import { pluralize } from '#lib/format.ts';
	import { clamp, ink, MONO, nameSize } from '../style.ts';
	import CardFrame from './CardFrame.svelte';

	/** An artist's link preview: their name, how many of the site's DJs played them, and their top three tracks. */
	let { name, djCount, tracks }: ArtistCardProps = $props();

	const size = $derived(nameSize(name, [[16, 104], [26, 80], [Infinity, 60]]));
</script>

<CardFrame>
	<div style="display: flex; flex-direction: column; justify-content: center; flex-grow: 1;">
		<div style="display: flex; font-family: {MONO}; font-size: 24px; color: {ink.fgMuted}; text-transform: uppercase; letter-spacing: 0.08em;">Artist</div>
		<div style="{clamp(2)} margin-top: 14px; font-size: {size}px; font-weight: 600; line-height: 1.02; letter-spacing: -0.035em;">{name}</div>
		<div style="display: flex; margin-top: 18px; font-size: 32px; color: {ink.fgSoft};">Played by {pluralize(djCount, 'DJ')} on Local Network</div>
		{#if tracks.length}
			<div style="display: flex; flex-direction: column; margin-top: 30px; border-top: 2px solid {ink.line};">
				{#each tracks.slice(0, 3) as track, index (index)}
					<div style="display: flex; align-items: center; padding-top: 14px;">
						<span style="width: 44px; flex-shrink: 0; font-family: {MONO}; font-size: 24px; color: {ink.fgSubtle};">{index + 1}</span>
						<span style="{clamp(1)} flex-grow: 1; flex-shrink: 1; font-size: 28px; color: {ink.fg};">{track.title}</span>
						<span style="flex-shrink: 0; margin-left: 24px; font-family: {MONO}; font-size: 24px; color: {ink.fgMuted}; white-space: nowrap;">{pluralize(track.supports, 'play')}</span>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</CardFrame>
