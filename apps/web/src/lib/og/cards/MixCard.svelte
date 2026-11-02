<script module lang="ts">
	import type { MixReport } from '#lib/mix/report.ts';

	/** What a mix's card shows. `cover` is a data URL the server made, or null for the performer's monogram. */
	export type MixCardProps = {
		title: string;
		performer: string;
		durationSecs: number;
		tracks: number;
		cover: string | null;
		waveform: Omit<MixReport['waveform'], 'secondsPerColumn'>;
	};

	/** About two pixels a column across the card: more is detail a preview can't show, and a heavier SVG to draw. */
	const COLUMNS = 528;

	/** `values` in at most `n` columns, each the loudest of the ones it takes, so peaks survive. */
	export function resample(values: number[], n: number): number[] {
		if (values.length <= n) return values;

		return Array.from({ length: n }, (_, i) =>
			Math.max(...values.slice(Math.floor((i * values.length) / n), Math.floor(((i + 1) * values.length) / n)))
		);
	}
</script>

<script lang="ts">
	import { clock } from '#lib/mix/report.ts';
	import { pluralize } from '#lib/format.ts';
	import Waveform from '#lib/ui/Waveform.svelte';
	import { CARD, clamp, ink, MONO, nameSize } from '../style.ts';
	import Artwork from './Artwork.svelte';
	import CardFrame from './CardFrame.svelte';

	/** A mix's link preview: its cover, title and performer, how long it runs and how many tracks, and its waveform. */
	let { title, performer, durationSecs, tracks, cover, waveform }: MixCardProps = $props();

	const size = $derived(nameSize(title, [[16, 76], [28, 62], [44, 50], [Infinity, 42]]));

	const bands = $derived({
		low: resample(waveform.low, COLUMNS),
		mid: resample(waveform.mid, COLUMNS),
		high: resample(waveform.high, COLUMNS)
	});
</script>

<CardFrame>
	<div style="display: flex; align-items: center;">
		<Artwork src={cover} name={performer} size={300} />
		<div style="display: flex; flex-direction: column; flex-grow: 1; flex-shrink: 1; min-width: 0; margin-left: 52px;">
			<div style="{clamp(2)} font-size: {size}px; font-weight: 600; line-height: 1.05; letter-spacing: -0.03em;">{title}</div>
			<div style="{clamp(1)} margin-top: 14px; font-size: 34px; color: {ink.fgSoft};">{performer}</div>
			<div style="display: flex; margin-top: 22px; font-family: {MONO}; font-size: 26px; color: {ink.fgMuted};">{clock(durationSecs)} · {pluralize(tracks, 'track')}</div>
		</div>
	</div>
	{#if bands.low.length}
		<div style="display: flex; width: {CARD.width - 144}px; height: 112px; margin-top: auto;">
			<Waveform {bands} class="" width={CARD.width - 144} height={112} />
		</div>
	{/if}
</CardFrame>
