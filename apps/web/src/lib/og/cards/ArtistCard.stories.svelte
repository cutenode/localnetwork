<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ArtistCard from './ArtistCard.svelte';
	import CardPreview from './CardPreview.svelte';
	import { longName } from './fixtures.ts';

	/** The link preview of an artist's page, drawn at 1200×630 and shown here scaled to fit. */
	const { Story } = defineMeta({ title: 'Link previews/Artist', component: ArtistCard, tags: ['autodocs'] });

	const tracks = [
		{ title: 'Night Bus', supports: 9 },
		{ title: 'Low Tide (Dub) with a remix credit too long for one line of the card', supports: 4 },
		{ title: 'Signal Path', supports: 1 }
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Top tracks" asChild>
	<CardPreview><ArtistCard name="Ferrule" djCount={12} {tracks} /></CardPreview>
</Story>

<Story name="One DJ, no tracks" asChild>
	<CardPreview><ArtistCard name="Mira Okonkwo" djCount={1} tracks={[]} /></CardPreview>
</Story>

<Story name="Long name" asChild>
	<CardPreview><ArtistCard name={longName} djCount={3} tracks={tracks.slice(0, 1)} /></CardPreview>
</Story>

<Story name="On a phone" asChild globals={phone}>
	<CardPreview><ArtistCard name="Ferrule" djCount={12} {tracks} /></CardPreview>
</Story>
