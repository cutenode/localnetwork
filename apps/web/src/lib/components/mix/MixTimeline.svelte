<script lang="ts">
	import { clock, colors, CONFIDENT, lanes, playColor, playingAt, timeStep, type MixReport } from '#lib/mix/report.ts';
	import { onColour } from '#lib/ui/contrast.ts';
	import Waveform from '#lib/ui/Waveform.svelte';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	const duration = $derived(Math.max(1, report.mix.durationSecs));

	const laneOf = $derived(lanes(report.plays));

	const laneCount = $derived(Math.max(2, ...laneOf.map((lane) => lane + 1)));

	const swatches = $derived(report.plays.map((_, i) => onColour(playColor(i))));

	const pct = (t: number) => `${(Math.min(Math.max(t, 0), duration) / duration) * 100}%`;

	let zoom = $state(1);

	let hover = $state<number | null>(null);

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

	const hoverText = $derived.by(() => {
		if (hover === null) return null;
		const on = playingAt(report, hover).map((i) => `${i + 1}. ${report.plays[i].track.title}`);

		return `${clock(hover)} · ${on.length ? on.join(' + ') : 'no track identified'}`;
	});

	function onpointermove(event: PointerEvent & { currentTarget: HTMLDivElement }) {
		const box = event.currentTarget.getBoundingClientRect();
		hover = ((event.clientX - box.left) / box.width) * duration;
	}
</script>

<div class="flex flex-col gap-3">
	<div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
		<ul class="m-0 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-label text-fg-muted" aria-label="Waveform colours">
			{#each [['Lows', colors.bands.low], ['Mids', colors.bands.mid], ['Highs', colors.bands.high]] as [name, colour] (name)}
				<li class="flex items-center gap-1.5"><span class="size-2.5 rounded-[2px]" style:background-color={colour}></span>{name}</li>
			{/each}
			<li class="flex items-center gap-1.5 max-sm:hidden"><span class="h-0.5 w-4 rounded-full bg-fg-soft"></span>How clearly each track is heard</li>
		</ul>
		<div class="flex gap-1 rounded-lg bg-well p-0.5" role="group" aria-label="Zoom">
			{#each [1, 2, 4] as z (z)}
				<button
					type="button"
					class={['cursor-pointer rounded-md px-2.5 py-1 font-mono text-caption', zoom === z ? 'bg-raised text-fg' : 'text-fg-muted hover:text-fg']}
					aria-pressed={zoom === z}
					onclick={() => (zoom = z)}>{z === 1 ? 'Fit' : `${z}×`}</button
				>
			{/each}
		</div>
	</div>

	<div class="overflow-x-auto">
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="relative flex flex-col gap-2" style:width="{zoom * 100}%" style:min-width="100%" {onpointermove} onpointerleave={() => (hover = null)}>
			<Waveform bands={report.waveform} label="The mix in three bands, {clock(duration)} long" class="h-24 sm:h-35" />

			<!-- On a phone the lanes are colour bars; the tracklist names them. -->
			<div class="relative [--lane:1rem] sm:[--lane:3.25rem]" style:height="calc({laneCount} * var(--lane))">
				{#each report.unidentified as gap (gap.start)}
					<div
						class="absolute flex h-[calc(var(--lane)-0.25rem)] items-center justify-center overflow-hidden rounded-sm border border-line-strong bg-well font-mono text-caption text-fg-subtle"
						style:left={pct(gap.start)}
						style:width={pct(gap.end - gap.start)}
						style:top="calc({(laneCount - 1) / 2} * var(--lane))"
						title="{clock(gap.start)} to {clock(gap.end)}: no track from the library found here"
					>
						<span class="truncate px-1 max-sm:hidden">?</span>
					</div>
				{/each}
				{#each report.plays as play, i (i)}
					{@const tentative = play.confidence < CONFIDENT}
					<button
						type="button"
						class={[
							'absolute flex h-[calc(var(--lane)-0.25rem)] cursor-pointer flex-col overflow-hidden rounded-sm px-2 text-left',
							tentative && 'opacity-60',
							selected === i && 'z-10 ring-2 ring-fg ring-offset-2 ring-offset-surface'
						]}
						style:left={pct(play.start)}
						style:width="max(0.75rem, {pct(play.end - play.start)})"
						style:top="calc({laneOf[i]} * var(--lane))"
						style:background-color={swatches[i].background}
						style:color={swatches[i].ink}
						aria-label="{i + 1}. {play.track.title}{play.track.artist ? ` by ${play.track.artist}` : ''}, {clock(play.start)} to {clock(play.end)}{tentative ? ', tentative' : ''}"
						aria-pressed={selected === i}
						onclick={() => (selected = selected === i ? null : i)}
					>
						<span class="relative z-10 flex min-w-0 items-baseline gap-1.5 pt-1 text-caption leading-tight font-semibold whitespace-nowrap max-sm:hidden">
							<span class="font-mono opacity-70">{i + 1}</span><span class="truncate">{play.track.title}</span>
						</span>
						<svg viewBox="0 0 100 40" preserveAspectRatio="none" class="absolute inset-x-0 bottom-0 h-7 w-full opacity-60 max-sm:hidden" aria-hidden="true">
							<path d={presencePath(play.presence)} class="fill-none stroke-current" stroke-width="1.5" vector-effect="non-scaling-stroke" shape-rendering="crispEdges" />
						</svg>
					</button>
				{/each}
			</div>

			<div class="relative h-5 font-mono text-caption text-fg-subtle" aria-hidden="true">
				{#each ticks as t, k (t)}
					<span class={['absolute top-0', k === 0 ? '' : t >= duration - 0.1 ? '-translate-x-full' : '-translate-x-1/2']} style:left={pct(t)}>{clock(t)}</span>
				{/each}
			</div>

			{#if hover !== null}
				<div class="pointer-events-none absolute top-0 bottom-7 w-px bg-playhead" style:left={pct(hover)} aria-hidden="true"></div>
			{/if}
		</div>
	</div>
	<p class="m-0 min-h-5 truncate text-label text-fg-muted" aria-live="polite">{#if hoverText}{hoverText}{:else}<span class="max-sm:hidden">Point at the mix to see what’s playing. Pick a track to find it in the tracklist.</span><span class="sm:hidden">Tap a track to find it in the tracklist.</span>{/if}</p>
</div>
