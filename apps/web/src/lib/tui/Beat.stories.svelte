<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn, playing } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** Where the playing deck is in the bar (its place in the beat grid), in four squares. */
	const { Story } = defineMeta({ title: 'Terminal/Beat', component: TuiStory, tags: ['tui'] });
</script>

<Story name="Beat 1" args={{ id: 'beat/one' }} play={drawn('■□□□')} />

<Story name="Beat 2" args={{ id: 'beat/two' }} play={drawn('□■□□')} />

<Story name="Beat 3" args={{ id: 'beat/three' }} play={drawn('□□■□')} />

<Story name="Beat 4" args={{ id: 'beat/four' }} play={drawn('□□□■')} />

<Story name="Two bars" args={{ id: 'beat/two-bars' }} play={playing} />
