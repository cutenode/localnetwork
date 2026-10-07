<script lang="ts">
	import { clock, niceTicks, speed, timeStep, type MixReport } from '#lib/mix/report.ts';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	const W = 600;

	const H = 220;

	const M = { l: 44, r: 10, t: 10, b: 28 };

	const duration = $derived(Math.max(1, report.mix.durationSecs));

	const rows = $derived(
		report.plays.flatMap((play, i) =>
			play.playedBpm !== undefined ? [{ i, play, played: play.playedBpm, recorded: play.track.bpm ?? play.playedBpm }] : []
		)
	);

	const scale = $derived.by(() => {
		const values = rows.flatMap((r) => [r.played, r.recorded]);
		let lo = Math.min(...values);
		let hi = Math.max(...values);
		const pad = Math.max(2, (hi - lo) * 0.12);
		lo -= pad;
		hi += pad;
		const ticks = niceTicks(lo, hi, 4);

		return { lo: Math.min(lo, ticks[0]), hi: Math.max(hi, ticks[ticks.length - 1]), ticks };
	});

	const x = (t: number) => M.l + (t / duration) * (W - M.l - M.r);

	const y = (v: number) => M.t + (1 - (v - scale.lo) / (scale.hi - scale.lo)) * (H - M.t - M.b);

	const played = $derived(rows.map((r, k) => `${k ? 'L' : 'M'}${x(r.play.start).toFixed(1)} ${y(r.played).toFixed(1)}H${x(r.play.end).toFixed(1)}`).join(''));

	const times = $derived.by(() => {
		const step = timeStep(duration, 6);
		const out: number[] = [];

		for (let t = 0; t <= duration; t += step) out.push(t);

		return out;
	});

	let hover = $state<number | null>(null);

	const shown = $derived(rows.find((r) => r.i === (hover ?? selected)));

	function onpointermove(event: PointerEvent & { currentTarget: SVGRectElement }) {
		const svg = event.currentTarget.ownerSVGElement ?? event.currentTarget;
		const box = svg.getBoundingClientRect();
		const t = ((((event.clientX - box.left) / box.width) * W - M.l) / (W - M.l - M.r)) * duration;
		let best = rows[0];
		let bestDistance = Infinity;

		for (const r of rows) {
			const d = t < r.play.start ? r.play.start - t : t > r.play.end ? t - r.play.end : 0;

			if (d < bestDistance) {
				bestDistance = d;
				best = r;
			}
		}

		hover = best?.i ?? null;
	}
</script>

{#if rows.length}
	<svg viewBox="0 0 {W} {H}" class="block w-full font-sans" role="img" aria-label="Tempo of each track as played and as recorded, across the mix">
		{#each scale.ticks as v (v)}
			<line x1={M.l} x2={W - M.r} y1={y(v)} y2={y(v)} class="stroke-ansi-blue" stroke-dasharray="4 4" />
			<text x={M.l - 6} y={y(v) + 5} text-anchor="end" font-size="16" class="fill-ansi-cyan">{v}</text>
		{/each}
		<line x1={M.l} x2={W - M.r} y1={H - M.b} y2={H - M.b} class="stroke-ansi-cyan" />
		{#each times as t (t)}
			<text x={x(t)} y={H - 8} text-anchor="middle" font-size="16" class="fill-ansi-cyan">{clock(t)}</text>
		{/each}
		{#each rows as r (r.i)}
			<line x1={x(r.play.start)} x2={x(r.play.end)} y1={y(r.recorded)} y2={y(r.recorded)} class="stroke-ansi-gray" stroke-dasharray="6 3" />
		{/each}
		<path d={played} class="fill-none stroke-ansi-yellow" stroke-width="3" shape-rendering="crispEdges" />
		{#if shown}
			<line x1={x(shown.play.start)} x2={x(shown.play.end)} y1={y(shown.played)} y2={y(shown.played)} class="stroke-ansi-white" stroke-width="5" />
		{/if}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<rect x={M.l} y="0" width={W - M.l - M.r} height={H - M.b} fill="transparent" {onpointermove} onpointerleave={() => (hover = null)} />
	</svg>
	<div class="mt-2 flex flex-wrap gap-x-4 text-sm">
		<span class="flex items-center gap-1.5"><span class="inline-block h-1 w-4 bg-ansi-yellow"></span>As played</span>
		<span class="flex items-center gap-1.5"><span class="inline-block h-0.5 w-4 bg-ansi-gray"></span>As recorded</span>
	</div>
	<p class="mt-1 min-h-6 text-sm text-ansi-cyan" aria-live="polite">
		{#if shown}
			{shown.i + 1}. {shown.play.track.title}: <span class="text-ansi-white">{shown.played.toFixed(1)} BPM</span>, recorded at {shown.recorded.toFixed(1)} ({speed(shown.play.tempo)})
		{:else}
			Point at the chart for each track’s tempo.
		{/if}
	</p>
{:else}
	<p class="muted text-sm">The library has no BPMs for these tracks.</p>
{/if}
