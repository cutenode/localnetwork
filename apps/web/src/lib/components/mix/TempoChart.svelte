<script lang="ts">
	import { bpmAt, clock, niceTicks, playColor, playSpeed, tempoSteps, timeStep, type MixReport } from '#lib/mix/report.ts';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	// The chart stretches to its box, so lines keep their width and labels are HTML beside it.
	const W = 1000;

	const H = 100;

	const duration = $derived(Math.max(1, report.mix.durationSecs));

	// Each play's BPM as played: one step, or one per speed where the pitch fader was ridden.
	const rows = $derived(
		report.plays.flatMap((play, i) => {
			const steps = tempoSteps(play);

			return steps.length ? [{ i, play, steps, recorded: play.track.bpm ?? steps[0].bpm }] : [];
		})
	);

	const scale = $derived.by(() => {
		const values = rows.flatMap((r) => [r.recorded, ...r.steps.map((step) => step.bpm)]);
		const pad = Math.max(2, (Math.max(...values) - Math.min(...values)) * 0.12);
		const lo = Math.min(...values) - pad;
		const hi = Math.max(...values) + pad;
		const ticks = niceTicks(lo, hi, 4);

		return { lo: Math.min(lo, ticks[0]), hi: Math.max(hi, ticks[ticks.length - 1]), ticks };
	});

	const x = (t: number) => (Math.min(Math.max(t, 0), duration) / duration) * W;

	const y = (v: number) => (1 - (v - scale.lo) / (scale.hi - scale.lo)) * H;

	const pct = (fraction: number) => `${fraction * 100}%`;

	const times = $derived.by(() => {
		const step = timeStep(duration, 4);
		const out: number[] = [];

		for (let t = 0; t < duration - step / 2; t += step) out.push(t);

		return out;
	});

	// The play pointed at, and where in the mix.
	let hover = $state<{ i: number; t: number } | null>(null);

	const shown = $derived(rows.find((r) => r.i === (hover?.i ?? selected)));

	// Its BPM where it's pointed at; else the one it was played at, or the slowest to the fastest.
	const shownBpm = $derived.by(() => {
		if (!shown) return '';

		const at = hover ? bpmAt(shown.steps, Math.min(Math.max(hover.t, shown.play.start), shown.play.end)) : undefined;
		const bpms = shown.steps.map((step) => step.bpm);

		if (at !== undefined) return `${at.toFixed(1)} BPM`;

		if (bpms.length > 1) return `${Math.min(...bpms).toFixed(1)} to ${Math.max(...bpms).toFixed(1)} BPM`;

		return `${bpms[0].toFixed(1)} BPM`;
	});

	function onpointermove(event: PointerEvent & { currentTarget: HTMLDivElement }) {
		const box = event.currentTarget.getBoundingClientRect();
		const t = ((event.clientX - box.left) / box.width) * duration;
		let best = rows[0];
		let bestDistance = Infinity;

		for (const r of rows) {
			const d = t < r.play.start ? r.play.start - t : t > r.play.end ? t - r.play.end : 0;

			if (d < bestDistance) {
				bestDistance = d;
				best = r;
			}
		}

		hover = best ? { i: best.i, t } : null;
	}
</script>

{#if rows.length}
	<div class="flex flex-col gap-1.5">
		<div class="relative mt-2 ml-9 h-36 sm:h-40">
			{#each scale.ticks as v (v)}
				<span class="absolute -left-9 w-7 -translate-y-1/2 text-right font-mono text-caption text-fg-subtle" style:top={pct(y(v) / H)}>{v}</span>
			{/each}
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="absolute inset-0" {onpointermove} onpointerleave={() => (hover = null)}>
				<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class="block size-full overflow-visible" role="img" aria-label="Tempo of each track as played and as recorded, across the mix">
					{#each scale.ticks as v (v)}
						<line x1="0" x2={W} y1={y(v)} y2={y(v)} class="stroke-line" vector-effect="non-scaling-stroke" />
					{/each}
					{#each rows as r (r.i)}
						{@const active = shown?.i === r.i}
						{#each r.steps as step (step.from)}
							<rect x={x(step.from)} y={y(step.bpm)} width={Math.max(1, x(step.to) - x(step.from))} height={Math.max(0, H - y(step.bpm))} class={active ? undefined : 'fill-line-strong'} style:fill={active ? playColor(r.i) : undefined} fill-opacity={active ? 0.45 : 0.6} />
							<line x1={x(step.from)} x2={x(step.to)} y1={y(step.bpm)} y2={y(step.bpm)} class="stroke-band-high" stroke-width="2" vector-effect="non-scaling-stroke" />
						{/each}
						<line x1={x(r.play.start)} x2={x(r.play.end)} y1={y(r.recorded)} y2={y(r.recorded)} class="stroke-fg-subtle" stroke-width="1.5" stroke-dasharray="4 3" vector-effect="non-scaling-stroke" />
					{/each}
					<line x1="0" x2={W} y1={H} y2={H} class="stroke-line-strong" vector-effect="non-scaling-stroke" />
				</svg>
			</div>
		</div>
		<div class="relative ml-9 h-4 font-mono text-caption text-fg-subtle" aria-hidden="true">
			{#each times as t, k (t)}<span class={['absolute top-0', k && '-translate-x-1/2']} style:left={pct(x(t) / W)}>{clock(t)}</span>{/each}
		</div>
	</div>
	<ul class="m-0 mt-3 flex list-none flex-wrap gap-x-4 p-0 text-label text-fg-muted">
		<li class="flex items-center gap-1.5"><span class="h-0.5 w-4 rounded-full bg-band-high"></span>BPM as played</li>
		<li class="flex items-center gap-1.5"><svg aria-hidden="true" viewBox="0 0 16 2" class="h-0.5 w-4"><line x1="0" x2="16" y1="1" y2="1" class="stroke-fg-subtle" stroke-width="2" stroke-dasharray="4 3" /></svg>BPM as recorded</li>
	</ul>
	<p class="m-0 mt-1 min-h-10 text-label text-fg-muted" aria-live="polite">
		{#if shown}
			{shown.i + 1}. {shown.play.track.title}: <span class="font-mono text-fg">{shownBpm}</span>, recorded at {shown.recorded.toFixed(1)} ({playSpeed(shown.play)})
		{:else}
			Point at the chart for each track’s tempo.
		{/if}
	</p>
{:else}
	<p class="m-0 text-label text-fg-muted">The library has no BPMs for these tracks.</p>
{/if}
