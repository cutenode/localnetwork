<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { sampleReport } from '#lib/mix/sample.ts';
	import Waveform from './Waveform.svelte';

	const overview = sampleReport.plays[0].track.overview!;

	const { Story } = defineMeta({
		title: 'Data/Waveform',
		component: Waveform,
		tags: ['autodocs'],
		argTypes: { align: { control: 'inline-radio', options: ['center', 'bottom'] }, playhead: { control: { type: 'range', min: 0, max: 1, step: 0.01 } } },
		args: { bands: sampleReport.waveform, label: 'Sunday Warm-up, 29 minutes', class: 'h-36' }
	});
</script>

<Story name="Three bands" />

<Story name="With playhead" args={{ playhead: 0.38 }} />

<Story name="Track overview" args={{ bands: undefined, heights: overview.heights, colors: overview.colors, label: 'Night Bus by Ferrule', class: 'h-16' }} />

<Story name="Bottom aligned" args={{ align: 'bottom', class: 'h-12' }} />

<Story name="Decorative" args={{ label: undefined, class: 'h-7 max-w-60' }} />
