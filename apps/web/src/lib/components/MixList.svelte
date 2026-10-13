<script lang="ts">
	import { ListMusic, Pause, Play } from '@lucide/svelte';
	import { embedUrl, mixPlatform } from '#lib/mix/links.ts';
	import type { ProjectMix } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import MixEmbed from './mix/MixEmbed.svelte';

	/** `tracklistHref` links mixes that have a tracklist to their page (by position, from 0). */
	let { mixes, tracklistHref }: { mixes: ProjectMix[]; tracklistHref?: (index: number) => string } = $props();

	/** The open player, by position and link, since records written elsewhere can repeat a link. */
	let open = $state<string | null>(null);

	const length = (minutes: number) => (minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h${minutes % 60 ? ` ${minutes % 60} min` : ''}`);
</script>

<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
	{#each mixes as mix, index (index)}
		{@const embed = embedUrl(mix.url)}
		{@const id = `${index} ${mix.url}`}
		{@const meta = [mixPlatform(mix.url), mix.durationMinutes && length(mix.durationMinutes), mix.year].filter(Boolean)}
		<Card as="li" padding="sm" class="flex flex-col gap-3">
			<div class="flex items-center gap-3.5">
				{#if embed}
					<button
						type="button"
						aria-label={open === id ? `Close the player for ${mix.title}` : `Play ${mix.title}`}
						aria-expanded={open === id}
						aria-controls="mix-player-{index}"
						class="grid size-10 shrink-0 cursor-pointer place-items-center rounded-full bg-fg text-fg-inverse transition-opacity hover:opacity-85"
						onclick={() => (open = open === id ? null : id)}
					>
						{#if open === id}<Pause class="size-4 fill-current" />{:else}<Play class="ml-0.5 size-4 fill-current" />{/if}
					</button>
				{/if}
				<div class="min-w-0 flex-1">
					<a class="block truncate text-emphasis font-semibold text-fg no-underline hover:underline" href={mix.url} rel="noopener noreferrer nofollow" target="_blank">{mix.title}</a>
					{#if meta.length}<p class="m-0 font-mono text-caption text-fg-subtle">{meta.join(' · ')}</p>{/if}
				</div>
				{#if mix.tracklist && tracklistHref}
					<Button variant="secondary" size="sm" href={tracklistHref(index)}><ListMusic class="size-4" />Tracklist</Button>
				{/if}
			</div>
			{#if embed && open === id}<MixEmbed id="mix-player-{index}" src={embed} title={mix.title} />{/if}
		</Card>
	{/each}
</ul>
