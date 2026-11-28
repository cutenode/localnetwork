<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The visualiser's bars, coloured by height, with peak caps. */
	const { Story } = defineMeta({ title: 'Terminal/Spectrum', component: TuiStory, tags: ['tui'] });
</script>

<Story name="Bars" args={{ id: 'spectrum/bars' }} play={drawn()} />

<Story name="Mirrored" args={{ id: 'spectrum/mirrored' }} play={drawn()} />
