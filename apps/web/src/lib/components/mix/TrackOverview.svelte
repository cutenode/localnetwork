<script lang="ts">
	import { bandColor, clock, nearestAnsi, phraseAt, type MixPlay } from '#lib/mix/report.ts';

	let { play }: { play: MixPlay } = $props();

	// Literal class names so Tailwind generates them.
	const FILL = new Map(
		Object.entries({
			'ansi-blue': 'fill-ansi-blue',
			'ansi-green': 'fill-ansi-green',
			'ansi-cyan': 'fill-ansi-cyan',
			'ansi-red': 'fill-ansi-red',
			'ansi-magenta': 'fill-ansi-magenta',
			'ansi-brown': 'fill-ansi-brown',
			'ansi-gray': 'fill-ansi-gray',
			'ansi-dark': 'fill-ansi-dark',
			'ansi-bright-blue': 'fill-ansi-bright-blue',
			'ansi-bright-green': 'fill-ansi-bright-green',
			'ansi-bright-cyan': 'fill-ansi-bright-cyan',
			'ansi-bright-red': 'fill-ansi-bright-red',
			'ansi-bright-magenta': 'fill-ansi-bright-magenta',
			'ansi-yellow': 'fill-ansi-yellow',
			'ansi-white': 'fill-ansi-white'
		})
	);

	const fill = (hex: string | undefined) => FILL.get(nearestAnsi(hex ?? '#ffffff')) ?? 'fill-ansi-gray';

	const BAND = { low: 'fill-ansi-bright-blue', mid: 'fill-ansi-bright-magenta', high: 'fill-ansi-white' } as const;

	const track = $derived(play.track);

	const total = $derived(Math.max(1, track.durationSecs || play.trackEnd));

	const n = $derived(track.overview?.heights.length || 100);

	const at = (secs: number) => (Math.min(Math.max(secs, 0), total) / total) * n;

	const lo = $derived(at(Math.min(play.trackStart, play.trackEnd)));

	const hi = $derived(at(Math.max(play.trackStart, play.trackEnd)));

	/** Bars grouped into one path per colour: the part that was played in colour, the rest dark. */
	const paths = $derived.by(() => {
		const groups = new Map<string, string>();
		const heights = track.overview?.heights;

		for (let x = 0; x < n; x++) {
			const h = heights ? (heights[x] / 255) * 32 : 6;

			if (h < 0.5) continue;
			const inside = x + 1 > lo && x < hi;
			const cls = inside ? (heights ? BAND[bandColor(track.overview!.colors[x])] : 'fill-ansi-cyan') : 'fill-ansi-dark';
			groups.set(cls, `${groups.get(cls) ?? ''}M${x} ${(34 - h).toFixed(1)}h0.8v${h.toFixed(1)}h-0.8z`);
		}

		return [...groups];
	});

	const into = $derived(phraseAt(track, play.trackStart));

	const outOf = $derived(phraseAt(track, play.trackEnd));
</script>

<div>
	<svg viewBox="0 0 {n} 44" preserveAspectRatio="none" class="block h-12 w-full" role="img" aria-label="Played {clock(play.trackStart)} to {clock(play.trackEnd)} of {clock(total)}" shape-rendering="crispEdges">
		{#each paths as [cls, d] (cls)}<path {d} class={cls} />{/each}
		{#each track.phrases as phrase, k (k)}
			<rect x={at(phrase.start)} y="38" width={Math.max(0.5, at(phrase.end) - at(phrase.start) - 0.6)} height="4" class={k % 2 ? 'fill-ansi-cyan' : 'fill-ansi-blue'}><title>{phrase.kind}</title></rect>
		{/each}
		{#each track.cues as cue (cue.label + cue.at)}
			<rect x={at(cue.at)} y="0" width={Math.max(0.8, n / 160)} height="5" class={fill(cue.color)}><title>Hot cue {cue.label} at {clock(cue.at)}</title></rect>
		{/each}
		<rect x={lo - 0.4} y="0" width="0.8" height="35" class="fill-ansi-yellow" />
		<rect x={hi - 0.4} y="0" width="0.8" height="35" class="fill-ansi-yellow" />
	</svg>
	<p class="flex flex-wrap justify-between gap-x-2 text-sm text-ansi-cyan" title="Played {clock(play.trackStart)} to {clock(play.trackEnd)} of {clock(total)}">
		<span class="whitespace-nowrap"><span class="text-ansi-white">{clock(play.trackStart)}</span> {into ?? 'in'}</span>
		<span class="whitespace-nowrap"><span class="text-ansi-white">{clock(play.trackEnd)}</span> {outOf ?? 'out'} <span class="text-ansi-dark">/ {clock(total)}</span></span>
	</p>
</div>
