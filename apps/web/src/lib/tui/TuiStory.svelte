<script lang="ts">
	import Cast from './Cast.svelte';
	import { PALETTES, capture, type StoryId } from './captures.ts';
	import Terminal from './Terminal.svelte';

	/** A locallibrary widget as its tests drew it, once in each palette, each on its own page colour. */
	let { id }: { id: StoryId } = $props();
</script>

<div class="flex flex-col gap-4">
	{#each PALETTES as palette (palette)}
		{#await capture(id, palette) then shown}
			{@const name = `${shown.widget} in the ${palette} palette`}
			<figure class="flex flex-col gap-2">
				<figcaption class="text-small text-fg-muted">{name}</figcaption>
				<!-- Wider than a phone, so it scrolls, and is focusable so a keyboard can scroll it. -->
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div
					class="w-fit max-w-full overflow-x-auto rounded p-2"
					style:background={shown.page.bg}
					role="region"
					aria-label={name}
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
