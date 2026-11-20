<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** A picture in half blocks, two pixels a cell, as cover art and the record are drawn. */
	const { Story } = defineMeta({ title: 'Terminal/Pixels', component: TuiStory });
</script>

<Story name="A gradient" args={{ id: 'pixels/gradient' }} play={drawn()} />
