<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { drawn } from './play.ts';
	import TuiStory from './TuiStory.svelte';

	/** The visualiser's oscilloscope trace in braille. */
	const { Story } = defineMeta({ title: 'Terminal/Scope', component: TuiStory, tags: ['tui'] });
</script>

<Story name="A tone" args={{ id: 'scope/tone' }} play={drawn()} />
