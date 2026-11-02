<script lang="ts">
	import type { ClassValue } from 'svelte/elements';

	/** Local Network's mark: five waveform bars in the band colours. It scales with its height. */
	let { class: extra = 'h-5' }: { class?: ClassValue } = $props();

	const bars = [
		[8, 'fill-band-low'],
		[18, 'fill-band-low'],
		[12, 'fill-band-mid'],
		[20, 'fill-band-high'],
		[10, 'fill-band-low']
	] as const;
</script>

<svg viewBox="0 0 23 20" aria-hidden="true" class={['w-auto shrink-0', extra]}>
	{#each bars as [height, fill], i (i)}<rect x={i * 5} y={(20 - height) / 2} width="3" {height} rx="1" class={fill} />{/each}
</svg>
