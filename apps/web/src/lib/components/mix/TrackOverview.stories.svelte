<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { MixPlay } from '#lib/mix/report.ts';
	import { sampleReport } from '#lib/mix/sample.ts';
	import TrackOverview from './TrackOverview.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Track overview', component: TrackOverview, tags: ['autodocs'] });

	const [play] = sampleReport.plays;

	/** A track the library has no waveform, cues or phrases for. */
	const bare: MixPlay = { ...play, track: { ...play.track, overview: undefined, cues: [], phrases: [] } };

	/** A short stretch from the middle of the track, in and out in the same phrase. */
	const excerpt: MixPlay = { ...play, trackStart: 120, trackEnd: 200 };
</script>

<!-- As wide as the tracklist's column for it. -->
<Story name="Played from its intro" asChild>
	<div class="w-52"><TrackOverview {play} /></div>
</Story>

<Story name="No overview" asChild>
	<div class="w-52"><TrackOverview play={bare} /></div>
</Story>

<Story name="A short stretch" asChild>
	<div class="w-52"><TrackOverview play={excerpt} /></div>
</Story>
