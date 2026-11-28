<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The scrolling waveform centred on the playhead, with beat ticks, cue lines and loops. */
	const { Story } = defineMeta({ title: 'Terminal/Detail', component: TuiStory, tags: ['tui'] });
</script>

<Story name="On a cue" args={{ id: 'detail/on-a-cue' }} play={drawn()} />

<Story name="In a loop" args={{ id: 'detail/in-a-loop' }} play={drawn()} />
