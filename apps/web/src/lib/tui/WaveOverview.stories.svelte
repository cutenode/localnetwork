<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The overview of a three-band waveform, with a second row for cue markers that collide. */
	const { Story } = defineMeta({ title: 'Terminal/Wave overview', component: TuiStory });
</script>

<Story name="Three bands" args={{ id: 'wave-overview/three-band' }} play={drawn('A', 'B', 'C')} />
