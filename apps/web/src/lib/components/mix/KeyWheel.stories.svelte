<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { sampleReport } from '#lib/mix/sample.ts';
	import KeyWheel from './KeyWheel.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Key wheel', component: KeyWheel, tags: ['autodocs'], args: { report: sampleReport } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const noKeys = { ...sampleReport, plays: sampleReport.plays.map((play) => ({ ...play, playedKey: undefined })) };
</script>

<Story name="Keys as played" />

<Story name="A key picked" args={{ selected: 2 }} />

<Story name="No keys" args={{ report: noKeys }} />

<!-- The keys in order, since the wheel doesn't fit. -->
<Story name="Phone" globals={phone} />

<Story name="Phone, no keys" args={{ report: noKeys }} globals={phone} />
