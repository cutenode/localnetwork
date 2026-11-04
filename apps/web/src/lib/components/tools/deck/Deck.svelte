<script lang="ts">
	import { ArrowRight } from '@lucide/svelte';
	import { bandColor, phraseBackground, type MixTrack } from '#lib/mix/report.ts';
	import type { Tool } from '#lib/tools/types.ts';
	import Waveform from '#lib/ui/Waveform.svelte';
	import Screen from './Screen.svelte';

	/**
	 * Every tool's hot cues on one track, the way a DJ marks a track before mixing it: each tool holds
	 * a stretch of the waveform, its cues lettered A onwards in its own colour. Each cue is a tab (a pad
	 * under the waveform), and the panel below shows its line beside the capture that proves it. Arrow
	 * keys, Home and End move along the track; a number or letter picks that cue in the tool's own run.
	 * `track.cues` is where each cue sits, in seconds, in the order the tools and their cues come.
	 */
	let { tools, track }: { tools: Pick<Tool, 'id' | 'name' | 'kind' | 'cues'>[]; track: Pick<MixTrack, 'durationSecs' | 'overview' | 'phrases'> & { cues: number[] } } = $props();

	const id = $props.id();

	const LETTERS = 'ABCDEFGH';

	// Each tool's colour, in the order they come: rekordbox's hot cue green, then a track colour that
	// stands out on the waveform's orange. Spelled out so Tailwind generates them. Every class here
	// lands in the stylesheet every page loads, so the deck sticks to ones the site already uses.
	const TONES = [
		{ text: 'text-cue-hot', bg: 'bg-cue-hot' },
		{ text: 'text-track-aqua', bg: 'bg-track-aqua' }
	];

	/** Every cue in track order, with its tool, that tool's colour, and where the tool's run starts. */
	const items = $derived(
		tools.flatMap((tool, t) => {
			const start = tools.slice(0, t).reduce((sum, { cues }) => sum + cues.length, 0);

			return tool.cues.map((cue, n) => ({ cue, tool, t, n, start, tone: TONES[t % TONES.length] }));
		})
	);

	const total = $derived(Math.max(1, track.durationSecs));

	const pct = (secs: number) => (Math.min(Math.max(secs, 0), total) / total) * 100;

	const points = $derived(track.cues.map(pct));

	const heights = $derived(track.overview?.heights ?? Array<number>(100).fill(96));

	const fills = $derived(track.overview?.colors.map(bandColor));

	let selected = $state(0);

	const item = $derived(items[selected]);

	const tabs: HTMLButtonElement[] = $state([]);

	function pick(k: number) {
		selected = (k + items.length) % items.length;
		tabs[selected]?.focus();
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.ctrlKey || event.metaKey || event.altKey) return;

		const { start, tool } = item;

		const moves = new Map([
			['ArrowRight', selected + 1],
			['ArrowLeft', selected - 1],
			['Home', 0],
			['End', items.length - 1]
		]);

		const isLetter = /^[a-z]$/i.test(event.key);

		// A cue's number or letter, counted within its tool's run, like a deck's own hot cue pads.
		const n = isLetter ? LETTERS.indexOf(event.key.toUpperCase()) : Number(event.key) - 1;

		const next = moves.get(event.key) ?? (n >= 0 && n < tool.cues.length ? start + n : undefined);

		// Letters are the site's shortcuts everywhere else; in here they belong to the deck.
		if (next === undefined && !isLetter) return;
		event.preventDefault();

		if (next !== undefined) pick(next);
	}
</script>

<section aria-labelledby="{id}-title" class="flex flex-col gap-10 sm:gap-14">
	<h2 id="{id}-title" class="sr-only">What the tools do</h2>

	<div class="flex flex-col gap-3">
		<!-- Each tool over its stretch of the track, as wide as its share of the cues, and the way to its page. -->
		<ul class="m-0 grid list-none gap-x-1.5 p-0" style:grid-template-columns={tools.map(({ cues }) => `${cues.length}fr`).join(' ')}>
			{#each tools as tool, t (tool.id)}
				<!-- Its name and kind describe each of its cues. -->
				<li id="{id}-tool-{t}" class="flex min-w-0 flex-col">
					<span aria-hidden="true" class={['mb-3 h-0.5', TONES[t % TONES.length].bg]}></span>
					<a href="/tools/{tool.id}" class="inline-flex items-center gap-1.5 self-start text-emphasis font-semibold text-fg no-underline hover:underline">
						{tool.name}
						<ArrowRight aria-hidden="true" class="size-4 text-fg-muted" />
					</a>
					<span class="truncate text-small text-fg-muted">{tool.kind}</span>
				</li>
			{/each}
		</ul>

		<div class="flex flex-col gap-1.5">
			<div aria-hidden="true" class="relative h-5">
				<!-- Each cue's flag, lettered as rekordbox letters hot cues. -->
				{#each items as { tone, n }, k (k)}
					<span
						class={['absolute top-0 grid size-5 -translate-x-1/2 place-items-center rounded-xs font-mono text-caption font-semibold', selected === k ? [tone.bg, 'text-fg-inverse'] : ['bg-well', tone.text]]}
						style:left="{points[k]}%">{LETTERS[n]}</span
					>
				{/each}
			</div>
			<div class="relative overflow-hidden">
				<Waveform {heights} colors={fills} align="bottom" label="The tools' hot cues on a track's waveform" class="h-20 sm:h-32" />
				{#each items as { tone }, k (k)}
					<span aria-hidden="true" class={['absolute inset-y-0 w-0.5 -translate-x-1/2', tone.bg, selected !== k && 'opacity-60']} style:left="{points[k]}%"></span>
				{/each}
				<!-- The playhead jumps to the picked cue. It moves by transform alone, so nothing reflows. -->
				<div aria-hidden="true" data-testid="playhead" class="pointer-events-none absolute inset-0 transition-transform duration-300 ease-out motion-reduce:transition-none" style:transform="translateX({points[selected]}%)">
					<span class="absolute inset-y-0 left-0 w-0.5 bg-playhead"></span>
				</div>
			</div>
			{#if track.phrases.length}
				<div aria-hidden="true" class="relative h-1.5">
					{#each track.phrases as phrase, p (p)}
						<span class="absolute inset-y-0 rounded-[1px] {phraseBackground(phrase.kind)}" style:left="{pct(phrase.start)}%" style:width="calc({pct(phrase.end) - pct(phrase.start)}% - 1px)"></span>
					{/each}
				</div>
			{/if}
		</div>

		<div role="tablist" aria-labelledby="{id}-title" class="mt-2 flex flex-wrap gap-1.5 xl:grid xl:grid-cols-12">
			{#each items as { cue, tone, t, n }, k (k)}
				<!-- On a narrow screen each tool's pads start a row of their own. -->
				{#if k > 0 && n === 0}<span aria-hidden="true" class="basis-full xl:hidden"></span>{/if}
				<button
					bind:this={tabs[k]}
					type="button"
					role="tab"
					id="{id}-tab-{k}"
					aria-selected={selected === k}
					aria-controls="{id}-panel"
					aria-describedby="{id}-tool-{t}"
					tabindex={selected === k ? 0 : -1}
					class={[
						'flex min-h-11 min-w-0 cursor-pointer items-center justify-center gap-2 rounded-md border px-3 text-left transition-colors motion-reduce:transition-none xl:flex-col xl:px-2',
						selected === k ? [tone.bg, 'border-transparent text-fg-inverse'] : 'border-line bg-surface text-fg-soft hover:border-line-strong hover:bg-raised'
					]}
					{onkeydown}
					onclick={() => (selected = k)}
					><span class={['font-mono text-caption font-semibold', selected !== k && tone.text]}>{LETTERS[n]}</span>
					<span class="max-w-full truncate text-small font-medium">{cue.name}</span></button
				>
			{/each}
		</div>
	</div>

	<div role="tabpanel" id="{id}-panel" aria-labelledby="{id}-tab-{selected}" class="grid grid-cols-[minmax(0,1fr)] gap-6 items-center lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-12">
		<div class="flex flex-col gap-4">
			<!-- Every cue's words are laid out in one cell, so the panel keeps the height of the longest. -->
			<div class="grid">
				{#each items as { cue, tool, tone, n }, k (k)}
					<div class={['flex flex-col gap-3 [grid-area:1/1]', selected !== k && 'invisible']} aria-hidden={selected !== k}>
						<h3 class="m-0 flex items-center gap-3 text-title font-semibold">
							<span aria-hidden="true" class={['grid size-8 shrink-0 place-items-center rounded-sm font-mono text-body font-semibold text-fg-inverse', tone.bg]}>{LETTERS[n]}</span>
							{cue.name}
						</h3>
						<p class="m-0 text-lede text-fg-soft">{cue.line}.</p>
						<a href="/tools/{tool.id}" class="link self-start text-body">See what {tool.name} does</a>
					</div>
				{/each}
			</div>
			<p class="m-0 hidden text-small text-fg-muted lg:block">Pick a cue, or press its letter.</p>
		</div>
		<Screen captures={items.map(({ cue }) => cue.capture)} {selected} />
	</div>
</section>
