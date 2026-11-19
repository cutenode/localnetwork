<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import { CARD, clamp, ink } from '../style.ts';
	import CardFrame from './CardFrame.svelte';
	import CardPreview from './CardPreview.svelte';
	import { photo } from './fixtures.ts';

	/** Every card's canvas and footer, drawn at 1200×630 and shown here scaled to fit. */
	const { Story } = defineMeta({ title: 'Link previews/Frame', component: CardFrame, tags: ['autodocs'] });
</script>

{#snippet title()}
	<div style="{clamp(2)} margin-top: auto; font-size: 76px; font-weight: 600; line-height: 1.05; letter-spacing: -0.03em;">Neon Tide</div>
	<div style="{clamp(1)} margin-top: 14px; font-size: 34px; color: {ink.fgSoft};">Solo DJ · Berlin</div>
{/snippet}

{#snippet background()}
	<img src={photo} alt="" width={CARD.width} height={CARD.height} style="position: absolute; top: 0; left: 0; width: {CARD.width}px; height: {CARD.height}px; object-fit: cover;" />
	<div style="display: flex; position: absolute; top: 0; left: 0; width: {CARD.width}px; height: {CARD.height}px; background-image: linear-gradient(to bottom, rgba(9, 9, 11, 0.1) 0%, rgba(9, 9, 11, 0.55) 45%, rgba(9, 9, 11, 0.94) 100%);"></div>
{/snippet}

<Story name="Frame" asChild>
	<CardPreview><CardFrame>{@render title()}</CardFrame></CardPreview>
</Story>

<Story name="Over a photo" asChild>
	<CardPreview><CardFrame {background}>{@render title()}</CardFrame></CardPreview>
</Story>

<Story name="On a phone" asChild globals={phone}>
	<CardPreview><CardFrame>{@render title()}</CardFrame></CardPreview>
</Story>
