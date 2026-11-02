<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { riddenReport, sampleReport } from '#lib/mix/sample.ts';
	import TempoChart from './TempoChart.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Tempo chart', component: TempoChart, tags: ['autodocs'], args: { report: sampleReport } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const noBpms = { ...sampleReport, plays: sampleReport.plays.map((play) => ({ ...play, playedBpm: undefined })) };
</script>

<Story name="Steady speeds" />

<Story name="Ridden pitch fader" args={{ report: riddenReport, selected: 1 }} />

<Story name="Ridden, on a phone" args={{ report: riddenReport, selected: 1 }} globals={phone} />

<Story name="No BPMs" args={{ report: noBpms }} />
