<script module lang="ts">
	/** What a performer's card shows. `avatar` is a data URL the server made, or null for their monogram. */
	export type PerformerCardProps = { name: string; kind: string; city: string; genres: string[]; avatar: string | null };
</script>

<script lang="ts">
	import { clamp, ink, MONO, nameSize } from '../style.ts';
	import Artwork from './Artwork.svelte';
	import CardFrame from './CardFrame.svelte';

	/** A performer's link preview: their picture, their name, what and where they are, and a few genres. */
	let { name, kind, city, genres, avatar }: PerformerCardProps = $props();

	const size = $derived(nameSize(name, [[12, 92], [20, 76], [32, 60], [Infinity, 48]]));
</script>

<CardFrame>
	<div style="display: flex; align-items: center; flex-grow: 1;">
		<Artwork src={avatar} {name} size={360} />
		<div style="display: flex; flex-direction: column; flex-grow: 1; flex-shrink: 1; min-width: 0; margin-left: 56px;">
			<div style="display: flex; font-size: 30px; color: {ink.fgMuted};">{[kind, city].filter(Boolean).join(' · ')}</div>
			<div style="{clamp(2)} margin-top: 12px; font-size: {size}px; font-weight: 600; line-height: 1.04; letter-spacing: -0.03em;">{name}</div>
			{#if genres.length}
				<div style="display: flex; flex-wrap: wrap; margin-top: 28px;">
					{#each genres.slice(0, 4) as genre (genre)}
						<span style="margin: 0 10px 10px 0; padding: 6px 16px; border: 2px solid {ink.lineStrong}; border-radius: 9999px; font-family: {MONO}; font-size: 22px; color: {ink.fgSoft};">{genre}</span>
					{/each}
				</div>
			{/if}
		</div>
	</div>
</CardFrame>
