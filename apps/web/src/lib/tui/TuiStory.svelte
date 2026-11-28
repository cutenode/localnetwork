<script lang="ts">
	import Cast from './Cast.svelte';
	import { PALETTES, capture, type Palette, type StoryId } from './captures.ts';
	import Terminal from './Terminal.svelte';

	/** A locallibrary widget as its tests drew it, once in each palette, each on its own page colour. */
	let { id }: { id: StoryId } = $props();

	// The story's title names the widget, so its caption only says which palette it's drawn in.
	const captions: Record<Palette, string> = { dark: 'Dark palette', light: 'Light palette' };
</script>

<div class="flex flex-col gap-4" data-tui-story>
	{#each PALETTES as palette (palette)}
		{#await capture(id, palette) then shown}
			<figure class="flex flex-col gap-2">
				<figcaption class="text-small text-fg-muted">{captions[palette]}</figcaption>
				<!-- Wider than a phone, so it scrolls, and is focusable so a keyboard can scroll it. The border shows
				     where the terminal ends when its page colour is the same as Storybook's. Square: a rounded corner's
				     antialiased edge isn't drawn the same way twice in the page CI screenshots, which an exact screenshot
				     comparison catches. -->
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div
					class="w-fit max-w-full overflow-x-auto border border-line p-2"
					style:background={shown.page.bg}
					role="region"
					aria-label={captions[palette]}
					tabindex="0"
				>
					{#if shown.moves}
						<Cast cast={shown.bytes} />
					{:else}
						<Terminal {...shown} />
					{/if}
				</div>
			</figure>
		{/await}
	{/each}
</div>
