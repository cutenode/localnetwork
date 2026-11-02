<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import { bandColor, phraseBackground, type MixTrack } from '#lib/mix/report.ts';
	import { cuePoints } from '#lib/tools/cues.ts';
	import type { Cue } from '#lib/tools/types.ts';
	import Waveform from '#lib/ui/Waveform.svelte';
	import Capture from './Capture.svelte';

	/**
	 * What a tool does, as hot cues on a real track's waveform: each cue is a tab, and its panel shows
	 * the cue's line beside the capture that proves it. Arrow keys, Home, End and the cue's number move
	 * between them, the way a player's hot cue pads jump.
	 */
	let { tool, cues, track, selected = $bindable(0) }: { tool: string; cues: Cue[]; track: Pick<MixTrack, 'durationSecs' | 'overview' | 'phrases'>; selected?: number } = $props();

	const id = $props.id();

	const LETTERS = 'ABCDEFGH';

	const total = $derived(Math.max(1, track.durationSecs));

	const pct = (secs: number) => (Math.min(Math.max(secs, 0), total) / total) * 100;

	const points = $derived(cuePoints(cues.length, total, track.phrases).map(pct));

	const heights = $derived(track.overview?.heights ?? Array<number>(100).fill(96));

	const fills = $derived(track.overview?.colors.map(bandColor));

	// Captures only load once their cue is picked, and the one showing stays up until the next has
	// loaded, so a slow image never leaves a gap. Each sits in a box of its own size from the start.
	const picked = new SvelteSet([0]);

	const loaded = new SvelteSet([0]);

	let showing = $state(0);

	$effect(() => {
		picked.add(selected);

		if (loaded.has(selected)) showing = selected;
	});

	function onload(i: number) {
		loaded.add(i);

		if (i === selected) showing = i;
	}

	const tabs: HTMLButtonElement[] = $state([]);

	function pick(i: number) {
		selected = (i + cues.length) % cues.length;
		tabs[selected]?.focus();
	}

	function onkeydown(event: KeyboardEvent) {
		const moves = new Map([
			['ArrowRight', selected + 1],
			['ArrowLeft', selected - 1],
			['Home', 0],
			['End', cues.length - 1]
		]);

		const number = Number(event.key);
		const next = moves.get(event.key) ?? (number >= 1 && number <= cues.length ? number - 1 : undefined);

		if (next === undefined) return;
		event.preventDefault();
		pick(next);
	}

	const cue = $derived(cues[selected]);
</script>

<section aria-labelledby="{id}-title" class="flex flex-col gap-8 sm:gap-10">
	<h2 id="{id}-title" class="sr-only">What {tool} does</h2>
	<div class="flex flex-col gap-1.5">
		<div role="tablist" aria-labelledby="{id}-title" class="relative h-8">
			{#each cues as item, i (item.name)}
				<button
					bind:this={tabs[i]}
					type="button"
					role="tab"
					id="{id}-tab-{i}"
					aria-selected={selected === i}
					aria-controls="{id}-panel"
					tabindex={selected === i ? 0 : -1}
					class={[
						'absolute top-0 flex h-7 cursor-pointer items-center gap-1.5 rounded-sm px-2 font-mono text-caption font-semibold whitespace-nowrap transition-colors',
						i === cues.length - 1 ? '-translate-x-full lg:translate-x-0' : 'max-lg:-translate-x-1/2',
						selected === i ? 'bg-cue-hot text-fg-inverse' : 'bg-well text-cue-hot hover:bg-raised'
					]}
					style:left="{points[i]}%"
					{onkeydown}
					onclick={() => (selected = i)}>{LETTERS[i]} <span class="max-lg:sr-only">{item.name}</span></button
				>
			{/each}
		</div>
		<div class="relative overflow-hidden">
			<Waveform {heights} colors={fills} align="bottom" label="{tool}'s hot cues on a track's waveform" class="h-20 sm:h-32" />
			{#each points as point, i (i)}
				<span aria-hidden="true" class={['absolute inset-y-0 w-0.5', selected === i ? 'bg-cue-hot' : 'bg-cue-hot/40']} style:left="{point}%"></span>
			{/each}
			<!-- The playhead jumps to the picked cue. It moves by transform alone, so nothing reflows. -->
			<div aria-hidden="true" data-testid="playhead" class="pointer-events-none absolute inset-0 transition-transform duration-300 ease-out motion-reduce:transition-none" style:transform="translateX({points[selected]}%)">
				<span class="absolute inset-y-0 left-0 w-0.5 bg-playhead"></span>
			</div>
		</div>
		{#if track.phrases.length}
			<div aria-hidden="true" class="relative h-1.5">
				{#each track.phrases as phrase, k (k)}
					<span class="absolute inset-y-0 rounded-[1px] {phraseBackground(phrase.kind)}" style:left="{pct(phrase.start)}%" style:width="calc({pct(phrase.end) - pct(phrase.start)}% - 1px)"></span>
				{/each}
			</div>
		{/if}
	</div>

	<div role="tabpanel" id="{id}-panel" aria-labelledby="{id}-tab-{selected}" class="grid grid-cols-[minmax(0,1fr)] items-center gap-6 lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-12">
		<div class="flex flex-col gap-3">
			<p class="m-0 font-mono text-small text-cue-hot">Hot cue {LETTERS[selected]}</p>
			<h3 class="m-0 text-title font-semibold">{cue.name}</h3>
			<p class="m-0 text-lede text-fg-soft">{cue.line}.</p>
			<p class="m-0 text-small text-fg-muted max-sm:hidden">Pick a cue, or press 1 to {cues.length} on one.</p>
		</div>
		<div class="grid grid-cols-[minmax(0,1fr)]">
			{#each cues as item, i (item.name)}
				{@const { w, h } = item.capture.picture.img}
				{@const phone = item.capture.phone?.img ?? { w, h }}
				<!-- Every cue's box is laid out, so the panel is as tall as its tallest capture from the start. -->
				<div
					class={['w-full justify-self-center [grid-area:1/1] aspect-(--phone) sm:aspect-(--wide)', showing !== i && 'invisible']}
					style:max-width="{w / 2}px"
					style:--wide="{w} / {h}"
					style:--phone="{phone.w} / {phone.h}"
					aria-hidden={showing !== i}
				>
					{#if picked.has(i)}
						<Capture capture={item.capture} eager={i === 0} sizes="(min-width: 1280px) 760px, (min-width: 1024px) 62vw, 100vw" onload={() => onload(i)} />
					{/if}
				</div>
			{/each}
		</div>
	</div>
</section>
