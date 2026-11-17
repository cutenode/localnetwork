<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { riddenReport, sampleReport } from '#lib/mix/sample.ts';
	import MixTracklist from './MixTracklist.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Tracklist', component: MixTracklist, tags: ['autodocs'], args: { report: sampleReport } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="A set" />

<Story name="A track picked" args={{ selected: 1 }} />

<!-- With the set player open, at its third track. -->
<Story name="Playing" args={{ playing: 2 }} />

<Story name="Ridden pitch fader" args={{ report: riddenReport }} />

<Story name="Phone" args={{ playing: 2 }} globals={phone} />
