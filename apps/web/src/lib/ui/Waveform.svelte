<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { colors, type MixReport } from '#lib/mix/report.ts';

	/**
	 * A waveform, values 0 to 255 as locallibrary exports them. Pass `bands` for a mix's
	 * three-band waveform: lows outermost, highs a bright core, each band in its colour at its
	 * token scale, like a player's. Or pass `heights` (with `colors`) for a track overview, one
	 * colour per column. Columns are grouped into one path per colour, so a long waveform stays a
	 * handful of nodes.
	 */
	let {
		bands,
		heights = [],
		colors: columnColors,
		align = 'center',
		playhead,
		label,
		class: extra = 'h-24'
	}: {
		bands?: Omit<MixReport['waveform'], 'secondsPerColumn'>;
		heights?: number[];
		colors?: string[];
		align?: 'center' | 'bottom';
		playhead?: number;
		label?: string;
		class?: ClassValue;
	} = $props();

	const H = 100;

	const n = $derived(Math.max(1, bands ? bands.low.length : heights.length));

	const bar = (x: number, width: number, value: number, scale: number) => {
		const h = (Math.min(Math.max(value, 0), 255) / 255) * (H - 2) * scale;

		if (h < 0.5) return '';

		return `M${x} ${(align === 'center' ? (H - h) / 2 : H - h).toFixed(1)}h${width}v${h.toFixed(1)}h-${width}z`;
	};

	const paths = $derived.by((): [string, string][] => {
		if (bands) {
			const { low, mid, high } = bands;
			const { scales } = colors.bands;
			const band = (values: number[], scale: number) => values.map((v, x) => bar(x, 1, v, scale)).join('');

			return [
				[colors.bands.low, band(low, scales.low)],
				[colors.bands.mid, band(mid, scales.mid)],
				[colors.bands.high, band(high, scales.high)]
			];
		}

		const groups = new Map<string, string>();

		heights.forEach((v, x) => {
			const fill = columnColors?.[x] ?? colors.bands.low;
			groups.set(fill, (groups.get(fill) ?? '') + bar(x, 0.8, v, 1));
		});

		return [...groups];
	});
</script>

<svg
	viewBox="0 0 {n} {H}"
	preserveAspectRatio="none"
	shape-rendering="crispEdges"
	class={['block w-full', extra]}
	role={label ? 'img' : undefined}
	aria-label={label}
	aria-hidden={label ? undefined : 'true'}
>
	{#each paths as [fill, d] (fill)}<path {d} style:fill={fill} />{/each}
	{#if playhead !== undefined}<rect x={Math.min(Math.max(playhead, 0), 1) * n - 0.3} y="0" width="0.6" height={H} style:fill={colors.playhead} />{/if}
</svg>
