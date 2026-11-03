<script module lang="ts">
	/** The hot cue pads' letters, so a tool has at most this many cues. */
	export const LETTERS = 'ABCDEFGH';

	/** How the track is drawn: rekordbox's coloured overview, or SoundCloud's flat bars with the part played lit. */
	export type Look = 'rekordbox' | 'soundcloud';

	const LOOKS = new Map<string, Look>([['localtoolbox', 'soundcloud']]);

	/** A tool's look, by its id: rekordbox's, unless the tool works somewhere else (localtoolbox on SoundCloud). */
	export const lookOf = (id: string): Look => LOOKS.get(id) ?? 'rekordbox';
</script>

<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import { bandColor, phraseBackground, type MixTrack } from '#lib/mix/report.ts';
	import { cuePoints } from '#lib/tools/cues.ts';
	import type { Cue } from '#lib/tools/types.ts';
	import Waveform from '#lib/ui/Waveform.svelte';
	import Capture from './Capture.svelte';

	/**
	 * What a tool does, as hot cues on a real track's waveform: each cue is a tab, and its panel shows
	 * the cue's line beside the capture that proves it. Arrow keys, Home, End and the cue's letter or
	 * number move between them, the way a player's hot cue pads jump.
	 *
	 * The cues' names sit on the track when there's room for them all (the container query). Below
	 * that the track shows only letters, which are just for pointing, and a legend under it holds the
	 * tabs. Only one of the two rows is ever displayed, so there's always exactly one tab list.
	 */
	let { tool, cues, track, look = 'rekordbox', selected = $bindable(0) }: { tool: string; cues: Cue[]; track: Pick<MixTrack, 'durationSecs' | 'overview' | 'phrases'>; look?: Look; selected?: number } = $props();

	const id = $props.id();

	const STYLES = {
		rekordbox: { on: 'bg-cue-hot-text text-fg-inverse', off: 'bg-well text-cue-hot-text hover:bg-raised', label: 'text-cue-hot-text' },
		soundcloud: { on: 'bg-fg text-fg-inverse', off: 'bg-well text-fg-soft hover:bg-raised', label: 'text-fg-muted' }
	};

	const style = $derived(STYLES[look]);

	const chip = 'flex h-7 cursor-pointer items-center gap-1.5 rounded-sm px-2 font-mono text-caption font-semibold whitespace-nowrap';

	const total = $derived(Math.max(1, track.durationSecs));

	const pct = (secs: number) => (Math.min(Math.max(secs, 0), total) / total) * 100;

	const points = $derived(cuePoints(cues.length, total, track.phrases).map(pct));

	const heights = $derived(track.overview?.heights ?? Array<number>(100).fill(96));

	const fills = $derived(look === 'soundcloud' ? heights.map(() => 'var(--color-fg-subtle)') : track.overview?.colors.map(bandColor));

	const played = $derived(heights.map(() => 'var(--color-track-orange)'));

	// Captures load only once their cue is picked or pointed at for a moment, so passing over cues on
	// the way to another loads nothing. The one showing stays up, dimmed, until the next has loaded.
	const DWELL = 150;

	const picked = new SvelteSet([0]);

	const loaded = new SvelteSet<number>();

	let showing = $state(0);

	const busy = $derived(showing !== selected);

	let timer: ReturnType<typeof setTimeout> | undefined;

	function load(i: number) {
		clearTimeout(timer);

		if (!picked.has(i)) timer = setTimeout(() => picked.add(i), DWELL);
	}

	$effect(() => {
		load(selected);

		if (loaded.has(selected)) showing = selected;

		return () => clearTimeout(timer);
	});

	function settle(i: number) {
		loaded.add(i);

		if (i === selected) showing = i;
	}

	const pointing = (i: number) => ({ onpointerenter: () => load(i), onpointerleave: () => load(selected) });

	function onkeydown(event: KeyboardEvent & { currentTarget: HTMLButtonElement }) {
		// Alt and Ctrl or Cmd with a key are the browser's (back, switching tabs).
		if (event.altKey || event.ctrlKey || event.metaKey) return;

		const moves = new Map([
			['ArrowRight', selected + 1],
			['ArrowLeft', selected - 1],
			['Home', 0],
			['End', cues.length - 1]
		]);

		const number = Number(event.key);
		const pad = LETTERS.indexOf(event.key.toUpperCase());
		const next = moves.get(event.key) ?? (pad >= 0 ? pad : number >= 1 && number <= cues.length ? number - 1 : undefined);

		if (next === undefined) return;
		// A pad's letter is never the site's shortcut here, even past the last cue.
		event.preventDefault();

		if (pad >= cues.length) return;
		selected = (next + cues.length) % cues.length;
		event.currentTarget.parentElement?.querySelectorAll<HTMLElement>('[role="tab"]')[selected]?.focus();
	}

	const tab = (i: number) => ({
		type: 'button' as const,
		role: 'tab' as const,
		'aria-selected': selected === i,
		'aria-controls': `${id}-panel`,
		tabindex: selected === i ? 0 : -1,
		onkeydown,
		onclick: () => (selected = i),
		...pointing(i)
	});

	const cue = $derived(cues[selected]);
</script>

<section aria-labelledby="{id}-title" class="@container flex flex-col gap-6 sm:gap-8">
	<div class="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
		<h2 id="{id}-title" class="m-0 section-title">What {tool} does</h2>
		<p class="m-0 text-small text-fg-muted"><span class="sm:hidden">Tap a cue.</span><span class="max-sm:hidden">Pick a cue, or press its letter or number.</span></p>
	</div>
	<div class="flex flex-col gap-1.5">
		<div role="tablist" aria-labelledby="{id}-title" class="relative hidden h-8 @min-[52rem]:block">
			{#each cues as item, i (item.name)}
				<!-- Kept inside the row: the last chip stops at its end. `ch` is a character of the chip's mono type. -->
				<button {...tab(i)} id="{id}-tab-{i}" class={[chip, 'absolute top-0', selected === i ? style.on : style.off]} style:left="min({points[i]}%, 100% - {item.name.length + 1}ch - 1.375rem)"
					>{LETTERS[i]} <span>{item.name}</span></button
				>
			{/each}
		</div>
		<!-- Letters only, for pointing: the legend below holds the tabs. Hidden too when even letters would collide. -->
		<div aria-hidden="true" class="relative h-8 @min-[52rem]:hidden @max-[17rem]:hidden">
			{#each cues as item, i (item.name)}
				<button type="button" tabindex="-1" class={[chip, 'absolute top-0 min-w-6 -translate-x-1/2 justify-center', selected === i ? style.on : style.off]} style:left="clamp(0.75rem, {points[i]}%, 100% - 0.75rem)" onclick={() => (selected = i)} {...pointing(i)}>{LETTERS[i]}</button>
			{/each}
		</div>
		<div class="relative overflow-hidden">
			<Waveform {heights} colors={fills} align="bottom" label="{tool}'s hot cues on a track's waveform" class="h-20 sm:h-32" />
			{#if look === 'soundcloud'}
				<!-- The part played up to the picked cue, revealed by clipping alone, so nothing reflows. -->
				<div aria-hidden="true" class="absolute inset-0 transition-[clip-path] duration-300 ease-out motion-reduce:transition-none" style:clip-path="inset(0 {100 - points[selected]}% 0 0)">
					<Waveform {heights} colors={played} align="bottom" class="h-20 sm:h-32" />
				</div>
				{#each points as point, i (i)}
					<span aria-hidden="true" class={['absolute bottom-1 size-2.5 -translate-x-1/2 rounded-full ring-2 ring-canvas', selected === i ? 'bg-track-orange' : 'bg-fg-muted']} style:left="{point}%"></span>
				{/each}
			{:else}
				{#each points as point, i (i)}
					<span aria-hidden="true" class={['absolute inset-y-0 ring-1 ring-canvas', selected === i ? 'w-0.5 bg-cue-hot' : 'w-px bg-cue-hot']} style:left="{point}%"></span>
				{/each}
				<!-- The playhead jumps to the picked cue. It moves by transform alone, so nothing reflows. -->
				<div aria-hidden="true" data-testid="playhead" class="pointer-events-none absolute inset-0 transition-transform duration-300 ease-out motion-reduce:transition-none" style:transform="translateX({points[selected]}%)">
					<span class="absolute inset-y-0 left-0 w-0.5 bg-playhead"></span>
				</div>
			{/if}
		</div>
		{#if look === 'rekordbox' && track.phrases.length}
			<div aria-hidden="true" class="relative h-1.5">
				{#each track.phrases as phrase, k (k)}
					<span class="absolute inset-y-0 rounded-[1px] {phraseBackground(phrase.kind)}" style:left="{pct(phrase.start)}%" style:width="calc({pct(phrase.end) - pct(phrase.start)}% - 1px)"></span>
				{/each}
			</div>
		{/if}
	</div>

	<div role="tablist" aria-labelledby="{id}-title" class="-mt-2 flex flex-wrap gap-1.5 sm:-mt-4 @min-[52rem]:hidden">
		{#each cues as item, i (item.name)}
			<button {...tab(i)} id="{id}-key-{i}" class={[chip, 'h-9 px-2.5', selected === i ? style.on : style.off]}>{LETTERS[i]} <span>{item.name}</span></button>
		{/each}
	</div>

	<div role="tabpanel" id="{id}-panel" tabindex="0" aria-labelledby="{id}-tab-{selected}" aria-busy={busy} class="grid grid-cols-[minmax(0,1fr)] items-center gap-6 rounded-xl lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-12">
		<div class="flex flex-col gap-3">
			<p class={['m-0 font-mono text-small', style.label]}>Hot cue {LETTERS[selected]}</p>
			<h3 class="m-0 text-title font-semibold">{cue.name}</h3>
			<p class="m-0 text-lede text-pretty text-fg-soft">{cue.line}.</p>
		</div>
		<!-- One stage of a fixed shape for each width, so a capture of any shape fits without moving anything. -->
		<div class="relative aspect-[39/70] overflow-hidden rounded-xl bg-well sm:aspect-[8/5]">
			{#each cues as item, i (item.name)}
				{#if picked.has(i)}
					{@const { w, h } = item.capture.picture.img}
					{@const phone = item.capture.phone?.img ?? { w, h }}
					<!-- Captures are taken at twice their size, so half is as large as one is shown. -->
					<div
						class={['absolute inset-0 transition-opacity duration-200 motion-reduce:duration-0', showing !== i && 'invisible', showing === i && busy && 'opacity-50 delay-200']}
						style:--wide-w="{w / 2}px"
						style:--wide-h="{h / 2}px"
						style:--phone-w="{phone.w / 2}px"
						style:--phone-h="{phone.h / 2}px"
						aria-hidden={showing !== i}
					>
						<Capture
							capture={item.capture}
							sizes="(min-width: 1280px) 760px, (min-width: 1024px) 62vw, 100vw"
							onsettle={() => settle(i)}
							class="absolute inset-0 m-auto size-full max-h-(--phone-h) max-w-(--phone-w) object-contain sm:max-h-(--wide-h) sm:max-w-(--wide-w)"
						/>
					</div>
				{/if}
			{/each}
		</div>
	</div>
</section>
