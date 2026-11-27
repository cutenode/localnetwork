<script module lang="ts">
	import { Download, ListMusic, LockOpen, Users } from '@lucide/svelte';

	/**
	 * localtoolbox's "pick a tool" menu as the extension draws it (apps/localtoolbox/src/lib/tools.ts):
	 * each tool's name, line, icon and tile colour, in the menu's order, keyed by the cue that shows
	 * it. A tool with no cue (the third-party downloader, until it has a capture) is left out.
	 * Spelled out so Tailwind generates the classes.
	 */
	export const MENU = [
		{ cue: 'Text', name: 'SoundCloud playlist to text', line: 'A playlist, likes or track page as text you can paste.', icon: ListMusic, tile: 'bg-band-mid' },
		{ cue: 'Downloads', name: 'Manager', line: 'List tracks from playlist, likes and track pages, and find their download links.', icon: Download, tile: 'bg-phrase-up' },
		{ cue: 'Downloader', name: 'third-party downloader', line: 'Walk the gate links behind a page and save the files.', icon: LockOpen, tile: 'bg-band-low' },
		{ cue: 'Artists', name: 'artists', line: 'Every artist behind your tracks, with their socials.', icon: Users, tile: 'bg-phrase-chorus' }
	];
</script>

<script lang="ts">
	import { ChevronRight } from '@lucide/svelte';
	import type { Cue } from '#lib/tools/types.ts';
	import Screen from './Screen.svelte';

	/**
	 * localtoolbox the way its toolbar menu shows it: its tools as the menu lists them, beside a capture
	 * of the one picked open over SoundCloud, then its marks on SoundCloud's pages. It's a list, not a
	 * deck, so it has no letters: the arrow keys, Home and End move along it, and the site's shortcuts
	 * stay free.
	 */
	let { cues, selected = $bindable(0) }: { cues: Cue[]; selected?: number } = $props();

	const id = $props.id();

	const items = $derived(MENU.flatMap((item) => cues.filter((cue) => cue.name === item.cue).map((cue) => ({ ...item, capture: cue.capture }))));

	const marks = $derived(cues.find((cue) => cue.name === 'Marks')?.capture);

	const tabs: HTMLButtonElement[] = $state([]);

	function onkeydown(event: KeyboardEvent) {
		if (event.ctrlKey || event.metaKey || event.altKey) return;

		const moves = new Map([
			['ArrowDown', selected + 1],
			['ArrowUp', selected - 1],
			['Home', 0],
			['End', items.length - 1]
		]);

		const next = moves.get(event.key);

		if (next === undefined) return;
		event.preventDefault();
		selected = (next + items.length) % items.length;
		tabs[selected]?.focus();
	}
</script>

<div class="grid grid-cols-[minmax(0,1fr)] items-center gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,8fr)] lg:gap-10">
	<div role="tablist" aria-orientation="vertical" aria-label="localtoolbox's tools" class="flex flex-col gap-2">
		{#each items as { name, line, icon: Glyph, tile }, i (name)}
			<button
				bind:this={tabs[i]}
				type="button"
				role="tab"
				id="{id}-tab-{i}"
				aria-selected={selected === i}
				aria-controls="{id}-panel"
				tabindex={selected === i ? 0 : -1}
				class={[
					'flex min-h-11 w-full cursor-pointer items-center gap-3 rounded-xl border p-3 text-left transition-colors motion-reduce:transition-none',
					selected === i ? 'border-fg bg-raised' : 'border-line bg-surface hover:border-line-strong'
				]}
				{onkeydown}
				onclick={() => (selected = i)}
			>
				<span aria-hidden="true" class={['grid size-10 shrink-0 place-items-center rounded-lg text-black', tile]}><Glyph class="size-5" /></span>
				<span class="flex min-w-0 flex-1 flex-col gap-0.5">
					<span class="text-body font-semibold text-fg">{name}</span>
					<!-- On a phone only the picked one's line shows. -->
					<span class={['text-body text-fg-muted', selected !== i && 'max-lg:hidden']}>{line}</span>
				</span>
				<ChevronRight aria-hidden="true" class={['size-4 shrink-0', selected === i ? 'text-fg' : 'text-fg-subtle']} />
			</button>
		{/each}
	</div>
	<div role="tabpanel" id="{id}-panel" aria-labelledby="{id}-tab-{selected}">
		<Screen captures={items.map(({ capture }) => capture)} {selected} />
	</div>
</div>

{#if marks}
	<!-- The marks aren't in the menu: they're drawn on SoundCloud's own pages, so they're shown apart. -->
	<div class="grid grid-cols-[minmax(0,1fr)] items-center gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,8fr)] lg:gap-10">
		<p class="m-0 text-lede text-fg-soft">On SoundCloud itself, it ticks the tracks you've downloaded and flags the ones with no download.</p>
		<Screen captures={[marks]} selected={0} />
	</div>
{/if}
