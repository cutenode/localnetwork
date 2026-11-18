<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn, playing } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** Where the playing deck is in the bar (its place in the beat grid), and the bar and beat. */
	const { Story } = defineMeta({ title: 'Terminal/Beat', component: TuiStory });
</script>

<Story name="Beat 1" args={{ id: 'beat/one' }} play={drawn('9.1')} />

<Story name="Beat 2" args={{ id: 'beat/two' }} play={drawn('9.2')} />

<Story name="Beat 3" args={{ id: 'beat/three' }} play={drawn('9.3')} />

<Story name="Beat 4" args={{ id: 'beat/four' }} play={drawn('9.4')} />

<Story name="Two bars" args={{ id: 'beat/two-bars' }} play={playing} />
