<script lang="ts">
	import { bandColor, clock, colors, phraseAt, phraseColor, type MixPlay } from '#lib/mix/report.ts';
	import Waveform from '#lib/ui/Waveform.svelte';

	let { play }: { play: MixPlay } = $props();

	const track = $derived(play.track);

	const total = $derived(Math.max(1, track.durationSecs || play.trackEnd));

	/** A track with no overview gets a flat bar, so the played range still shows. */
	const heights = $derived(track.overview?.heights ?? Array<number>(100).fill(48));

	const n = $derived(heights.length);

	const at = (secs: number) => (Math.min(Math.max(secs, 0), total) / total) * n;

	const pct = (secs: number) => `${(at(secs) / n) * 100}%`;

	const lo = $derived(at(Math.min(play.trackStart, play.trackEnd)));

	const hi = $derived(at(Math.max(play.trackStart, play.trackEnd)));

	// The part played in the mix in the band colours, the rest dimmed.
	const fills = $derived(heights.map((_, x) => (x + 1 > lo && x < hi ? (track.overview ? bandColor(track.overview.colors[x]) : colors.bands.high) : 'var(--color-line-strong)')));

	const into = $derived(phraseAt(track, play.trackStart));

	const outOf = $derived(phraseAt(track, play.trackEnd));

	const range = $derived(`Played ${clock(play.trackStart)} to ${clock(play.trackEnd)} of ${clock(total)}`);
</script>

<div class="flex flex-col gap-1">
	<div class="relative pt-1.5">
		{#each track.cues as cue (cue.label + cue.at)}
			<span class="absolute top-0 h-1.5 w-0.5" style:left={pct(cue.at)} style:background-color={cue.color ?? colors.cues.hot} title="Hot cue {cue.label} at {clock(cue.at)}"></span>
		{/each}
		<Waveform {heights} colors={fills} align="bottom" label={range} class="h-8" />
		<span aria-hidden="true" class="absolute bottom-0 w-px bg-playhead" style:left={pct(Math.min(play.trackStart, play.trackEnd))} style:top="0.375rem"></span>
		<span aria-hidden="true" class="absolute bottom-0 w-px bg-playhead" style:left={pct(Math.max(play.trackStart, play.trackEnd))} style:top="0.375rem"></span>
	</div>
	{#if track.phrases.length}
		<div class="relative h-1">
			{#each track.phrases as phrase, k (k)}
				<span class="absolute inset-y-0 rounded-[1px]" style:left={pct(phrase.start)} style:width="calc({pct(phrase.end - phrase.start)} - 1px)" style:background-color={phraseColor(phrase.kind)} title={phrase.kind}></span>
			{/each}
		</div>
	{/if}
	<p class="m-0 flex flex-wrap justify-between gap-x-2 font-mono text-caption text-fg-subtle" title={range}>
		<span class="whitespace-nowrap"><span class="text-fg-soft">{clock(play.trackStart)}</span> {into ?? 'in'}</span>
		<span class="whitespace-nowrap"><span class="text-fg-soft">{clock(play.trackEnd)}</span> {outOf ?? 'out'} / {clock(total)}</span>
	</p>
</div>
