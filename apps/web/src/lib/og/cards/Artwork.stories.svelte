<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Artwork from './Artwork.svelte';
	import { avatar, longName } from './fixtures.ts';

	/** A card's square picture, or the name's monogram in its own colour when there's none. */
	const { Story } = defineMeta({ title: 'Link previews/Artwork', component: Artwork, tags: ['autodocs'] });
</script>

<Story name="Picture" args={{ src: avatar, name: 'Neon Tide', size: 300 }} />

<Story name="Monogram" args={{ src: null, name: 'Neon Tide', size: 300 }} />

<Story name="Monogram, long name" args={{ src: null, name: longName, size: 300 }} />

<Story name="Small" args={{ src: null, name: 'Ferrule', size: 96 }} />
