<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The phrase the playing deck is in, and its bar of the phrase's bars. */
	const { Story } = defineMeta({ title: 'Terminal/Phrase', component: TuiStory });
</script>

<Story name="In the intro" args={{ id: 'phrase/intro' }} play={drawn('Intro', '3/8')} />

<Story name="In the bridge" args={{ id: 'phrase/bridge' }} play={drawn('Bridge', '5/8')} />
