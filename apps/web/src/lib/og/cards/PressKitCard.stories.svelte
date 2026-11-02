<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CardPreview from './CardPreview.svelte';
	import { longName, performer, photo } from './fixtures.ts';
	import PressKitCard from './PressKitCard.svelte';

	/** The link preview of a press kit, drawn at 1200×630 and shown here scaled to fit. */
	const { Story } = defineMeta({ title: 'Link previews/Press kit', component: PressKitCard, tags: ['autodocs'] });

	const headline = 'Rolling techno for late rooms, from warm-up to close';

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="With a press photo" asChild>
	<CardPreview><PressKitCard {performer} {headline} {photo} /></CardPreview>
</Story>

<Story name="No headline, long name" asChild>
	<CardPreview><PressKitCard performer={{ ...performer, name: longName }} headline={null} {photo} /></CardPreview>
</Story>

<Story name="No photo" asChild>
	<CardPreview><PressKitCard {performer} {headline} photo={null} /></CardPreview>
</Story>

<Story name="On a phone" asChild globals={phone}>
	<CardPreview><PressKitCard {performer} {headline} {photo} /></CardPreview>
</Story>
