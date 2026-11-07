<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CardPreview from './CardPreview.svelte';
	import { avatar, longName, waveform } from './fixtures.ts';
	import MixCard from './MixCard.svelte';

	/** The link preview of a mix's tracklist, drawn at 1200×630 and shown here scaled to fit. */
	const { Story } = defineMeta({ title: 'Link previews/Mix', component: MixCard, tags: ['autodocs'] });

	const mix = { title: 'Late Room', performer: 'Neon Tide', durationSecs: 3723, tracks: 31, cover: avatar, waveform };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="With a cover" asChild>
	<CardPreview><MixCard {...mix} /></CardPreview>
</Story>

<Story name="No cover or picture" asChild>
	<CardPreview><MixCard {...mix} cover={null} /></CardPreview>
</Story>

<Story name="Long title, one track" asChild>
	<CardPreview><MixCard {...mix} title="Live from the basement of a warehouse at the far end of the harbour, part two" performer={longName} durationSecs={600} tracks={1} /></CardPreview>
</Story>

<Story name="On a phone" asChild globals={phone}>
	<CardPreview><MixCard {...mix} /></CardPreview>
</Story>
