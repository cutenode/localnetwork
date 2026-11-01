<script module lang="ts">
	import type { PerformerCardProps } from './PerformerCard.svelte';

	/** What a press kit's card shows: its first public photo (a data URL the server made), or the performer's card. */
	export type PressKitCardProps = { performer: PerformerCardProps; headline: string | null; photo: string | null };
</script>

<script lang="ts">
	import { CARD, clamp, ink, nameSize } from '../style.ts';
	import CardFrame from './CardFrame.svelte';
	import PerformerCard from './PerformerCard.svelte';

	/**
	 * A press kit's link preview: its first public photo edge to edge, darkened from below so the performer's name and
	 * the kit's headline read over it. A kit without one shows the performer's card.
	 */
	let { performer, headline, photo }: PressKitCardProps = $props();

	const size = $derived(nameSize(performer.name, [[18, 96], [28, 76], [Infinity, 60]]));
</script>

{#if photo}
	<CardFrame>
		{#snippet background()}
			<img src={photo} alt="" width={CARD.width} height={CARD.height} style="position: absolute; top: 0; left: 0; width: {CARD.width}px; height: {CARD.height}px; object-fit: cover;" />
			<div style="display: flex; position: absolute; top: 0; left: 0; width: {CARD.width}px; height: {CARD.height}px; background-image: linear-gradient(to bottom, rgba(9, 9, 11, 0.1) 0%, rgba(9, 9, 11, 0.55) 45%, rgba(9, 9, 11, 0.94) 100%);"></div>
		{/snippet}
		<div style="display: flex; flex-direction: column; justify-content: flex-end; flex-grow: 1; max-width: 960px;">
			<div style="{clamp(2)} font-size: {size}px; font-weight: 600; line-height: 1.02; letter-spacing: -0.03em;">{performer.name}</div>
			{#if headline}<div style="{clamp(2)} margin-top: 16px; font-size: 32px; line-height: 1.3; color: {ink.fgSoft};">{headline}</div>{/if}
		</div>
	</CardFrame>
{:else}
	<PerformerCard {...performer} />
{/if}
