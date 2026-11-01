<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CardPreview from './CardPreview.svelte';
	import { longName, performer } from './fixtures.ts';
	import PerformerCard from './PerformerCard.svelte';

	/** The link preview of a performer's pages, drawn at 1200×630 and shown here scaled to fit. */
	const { Story } = defineMeta({ title: 'Link previews/Performer', component: PerformerCard, tags: ['autodocs'] });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="With a picture" asChild>
	<CardPreview><PerformerCard {...performer} /></CardPreview>
</Story>

<Story name="No picture, no genres" asChild>
	<CardPreview><PerformerCard {...performer} avatar={null} genres={[]} /></CardPreview>
</Story>

<Story name="Long name" asChild>
	<CardPreview><PerformerCard {...performer} name={longName} kind="Collective / crew" avatar={null} /></CardPreview>
</Story>

<Story name="On a phone" asChild globals={phone}>
	<CardPreview><PerformerCard {...performer} /></CardPreview>
</Story>
