<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The cues around the playhead on one rail: hot cues lettered, memory cues as diamonds, loops as bars, names under them. */
	const { Story } = defineMeta({ title: 'Terminal/Cue lane', component: TuiStory });
</script>

<Story name="In the bridge" args={{ id: 'cue-lane/bridge' }} play={drawn('B', 'Bridge')} />
