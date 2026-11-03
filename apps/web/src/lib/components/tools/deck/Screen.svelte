<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import type { Capture as CaptureData } from '#lib/tools/types.ts';
	import Capture from '../Capture.svelte';

	/**
	 * The deck's screen: a box of one fixed shape, so nothing moves when the picked capture changes,
	 * with each capture fitted inside it. A capture loads once it's picked, and the one showing stays
	 * up until the next has loaded, so switching never flashes an empty screen. The first one counts
	 * as loaded, since the page opens on it.
	 */
	let { captures, selected }: { captures: CaptureData[]; selected: number } = $props();

	const picked = new SvelteSet([0]);

	const loaded = new SvelteSet([0]);

	let showing = $state(0);

	$effect.pre(() => {
		picked.add(selected);

		if (loaded.has(selected)) showing = selected;
	});

	function onload(i: number) {
		loaded.add(i);

		if (i === selected) showing = i;
	}
</script>

<div class="grid aspect-(--phone) overflow-hidden rounded-2xl border border-line bg-sunken p-2 sm:aspect-(--wide)" style:--phone="39 / 70" style:--wide="16 / 10">
	{#each captures as capture, i (i)}
		{#if picked.has(i)}
			<div class={['relative [grid-area:1/1]', showing !== i && 'invisible']} aria-hidden={showing !== i}>
				<Capture {capture} class="absolute inset-0 h-full object-contain" sizes="(min-width: 1280px) 780px, (min-width: 1024px) 62vw, 100vw" onload={() => onload(i)} />
			</div>
		{/if}
	{/each}
</div>
