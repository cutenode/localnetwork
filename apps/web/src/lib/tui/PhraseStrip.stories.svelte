<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn, playing } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** rekordbox's phrases across the whole track, the played part dimmed. */
	const { Story } = defineMeta({ title: 'Terminal/Phrase strip', component: TuiStory, tags: ['tui'] });
</script>

<Story name="In the intro" args={{ id: 'phrase-strip/intro' }} play={drawn('Intro', 'Bridge', 'Outro')} />

<Story name="In the bridge" args={{ id: 'phrase-strip/bridge' }} play={drawn('Intro', 'Bridge', 'Outro')} />

<Story name="Into the verse" args={{ id: 'phrase-strip/into-the-verse' }} play={playing} />
