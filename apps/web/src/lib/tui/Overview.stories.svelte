<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** loclib-waveform's whole-track overview: cue markers over the waveform, the played part dimmed and the playhead. */
	const { Story } = defineMeta({ title: 'Terminal/Overview', component: TuiStory });
</script>

<Story name="At the start" args={{ id: 'overview/start' }} play={drawn('A', 'B', 'C')} />

<Story name="In the bridge" args={{ id: 'overview/bridge' }} play={drawn('A', 'B', 'C')} />
