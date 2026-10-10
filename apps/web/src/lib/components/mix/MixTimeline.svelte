<script lang="ts">
	import { clock, CONFIDENT, lanes, playingAt, timeStep, type MixReport } from '#lib/mix/report.ts';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	const duration = $derived(Math.max(1, report.mix.durationSecs));

	const laneOf = $derived(lanes(report.plays));

	const laneCount = $derived(Math.max(2, ...laneOf.map((lane) => lane + 1)));

	const pct = (t: number) => `${(Math.min(Math.max(t, 0), duration) / duration) * 100}%`;

	// One path per band: lows outermost, highs a bright core, like a player's 3-band waveform.
	const HEIGHT = 100;

	const bands = $derived.by(() => {
		const w = report.waveform;
		const n = w.low.length;

		const band = (values: number[], scale: number) => {
			let d = '';

			for (let x = 0; x < n; x++) {
				const amp = (values[x] / 255) * (HEIGHT / 2 - 1) * scale;

				if (amp < 0.5) continue;
				d += `M${x} ${(HEIGHT / 2 - amp).toFixed(1)}h1v${(amp * 2).toFixed(1)}h-1z`;
			}

			return d;
		};

		return { n, low: band(w.low, 1), mid: band(w.mid, 0.66), high: band(w.high, 0.32) };
	});

	const ticks = $derived.by(() => {
		const step = timeStep(duration, zoom * 7);
		const out: number[] = [];

		for (let t = 0; t <= duration + 0.1; t += step) out.push(t);

		return out;
	});

	/** A stepped line of how clearly the track is heard, in its block's own 0-100 space. */
	function presencePath(values: number[]): string {
		if (!values.length) return 'M0 30H100';
		const step = 100 / values.length;
		const y = (v: number) => (36 - v * 0.3).toFixed(1);

		return values.map((v, i) => `${i ? `V${y(v)}` : `M0 ${y(v)}`}H${((i + 1) * step).toFixed(2)}`).join('');
	}

	let zoom = $state(1);

	let hover = $state<number | null>(null);

	const hoverText = $derived.by(() => {
		if (hover === null) return null;
		const on = playingAt(report, hover).map((i) => `${i + 1}. ${report.plays[i].track.title}`);

		return `${clock(hover)} · ${on.length ? on.join('  +  ') : 'no track identified'}`;
	});

	function onpointermove(event: PointerEvent & { currentTarget: HTMLDivElement }) {
		const box = event.currentTarget.getBoundingClientRect();
		hover = ((event.clientX - box.left) / box.width) * duration;
	}
</script>

<div class="space-y-2">
	<div class="flex flex-wrap items-center justify-between gap-2 text-sm">
		<ul class="flex flex-wrap gap-x-4 gap-y-1" aria-label="Waveform colours">
			<li class="flex items-center gap-1.5"><span class="inline-block size-3 bg-ansi-bright-blue"></span>Lows</li>
			<li class="flex items-center gap-1.5"><span class="inline-block size-3 bg-ansi-bright-magenta"></span>Mids</li>
			<li class="flex items-center gap-1.5"><span class="inline-block size-3 bg-ansi-white"></span>Highs</li>
			<li class="flex items-center gap-1.5"><span class="inline-block h-0.5 w-4 bg-ansi-bright-green"></span>How clearly each track is heard</li>
		</ul>
		<div class="flex gap-1" role="group" aria-label="Zoom">
			{#each [1, 2, 4] as z (z)}
				<button type="button" class="btn-ghost btn-sm" aria-pressed={zoom === z} class:bg-ansi-blue={zoom === z} onclick={() => (zoom = z)}>{z === 1 ? 'Fit' : `${z}×`}</button>
			{/each}
		</div>
	</div>

	<div class="overflow-x-auto border-2 border-ansi-blue">
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="relative" style="width: {zoom * 100}%; min-width: 100%" {onpointermove} onpointerleave={() => (hover = null)}>
			<svg viewBox="0 0 {bands.n} {HEIGHT}" preserveAspectRatio="none" class="block h-28 w-full" role="img" aria-label="The mix's loudness in three bands, {clock(duration)} long" shape-rendering="crispEdges">
				<path d={bands.low} class="fill-ansi-bright-blue" />
				<path d={bands.mid} class="fill-ansi-bright-magenta" />
				<path d={bands.high} class="fill-ansi-white" />
			</svg>

			<div class="relative border-t-2 border-dashed border-ansi-blue" style="height: {laneCount * 3.75}rem">
				{#each report.unidentified as gap (gap.start)}
					<div class="absolute flex h-14 items-center justify-center overflow-hidden border-2 border-dashed border-ansi-dark text-sm text-ansi-dark" style="left: {pct(gap.start)}; width: {pct(gap.end - gap.start)}; top: {((laneCount - 1) * 3.75) / 2 + 0.125}rem" title="{clock(gap.start)}–{clock(gap.end)}: no track from the library found here">
						<span class="truncate px-1">??</span>
					</div>
				{/each}
				{#each report.plays as play, i (i)}
					<button
						type="button"
						class="absolute flex h-14 flex-col overflow-hidden border-2 bg-ansi-black px-1 text-left {play.confidence < CONFIDENT ? 'border-dashed border-ansi-dark' : selected === i ? 'border-ansi-yellow' : 'border-ansi-cyan hover:border-ansi-bright-cyan'}"
						style="left: {pct(play.start)}; width: max(0.75rem, {pct(play.end - play.start)}); top: {laneOf[i] * 3.75 + 0.125}rem"
						aria-label="{i + 1}. {play.track.title}{play.track.artist ? ` by ${play.track.artist}` : ''}, {clock(play.start)} to {clock(play.end)}"
						aria-pressed={selected === i}
						onclick={() => (selected = selected === i ? null : i)}
					>
						<span class="relative z-10 flex min-w-0 items-baseline gap-1.5 text-sm leading-none whitespace-nowrap">
							<span class="text-ansi-yellow">{i + 1}</span><span class="truncate text-ansi-white">{play.track.title}</span>
						</span>
						<svg viewBox="0 0 100 40" preserveAspectRatio="none" class="absolute inset-x-0 bottom-0 h-9 w-full" aria-hidden="true">
							<path d={presencePath(play.presence)} class="fill-none stroke-ansi-bright-green" stroke-width="2" vector-effect="non-scaling-stroke" shape-rendering="crispEdges" />
						</svg>
					</button>
				{/each}
			</div>

			<div class="relative h-6 border-t-2 border-ansi-blue text-sm text-ansi-cyan" aria-hidden="true">
				{#each ticks as t (t)}
					<span class="absolute top-0.5 -translate-x-1/2 first:translate-x-0" style="left: {pct(t)}">{clock(t)}</span>
				{/each}
			</div>

			{#if hover !== null}
				<div class="pointer-events-none absolute top-0 bottom-6 w-0.5 bg-ansi-yellow" style="left: {pct(hover)}" aria-hidden="true"></div>
			{/if}
		</div>
	</div>
	<p class="min-h-6 truncate text-sm text-ansi-cyan" aria-live="polite">{hoverText ?? 'Point at the mix for what’s playing when; pick a track to find it in the tracklist.'}</p>
</div>
