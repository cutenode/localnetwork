<script module lang="ts">
	/**
	 * Section colours, all borrowed from DJ data so the page has no brand accent of its own:
	 * waveform bands, a phrase and a track colour. Each reads at 4.5:1 or better on canvas.
	 */
	export const tones = {
		low: 'text-band-low',
		mid: 'text-band-mid',
		high: 'text-band-high',
		chorus: 'text-phrase-chorus',
		aqua: 'text-track-aqua',
		link: 'text-link',
		muted: 'text-fg-subtle'
	} as const;

	export type Tone = keyof typeof tones;
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';

	let { tone = 'muted', children }: { tone?: Tone; children: Snippet } = $props();
</script>

<p class="eyebrow m-0 {tones[tone]}">{@render children()}</p>
