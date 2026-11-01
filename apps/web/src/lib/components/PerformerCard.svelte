<script lang="ts">
	import type { Snippet } from 'svelte';
	import { formatDistance } from '#lib/geo.ts';
	import type { PerformerSummary } from '#lib/performers.ts';
	import Avatar from './Avatar.svelte';

	/** A performer in a list: a project or a listed DJ, from projectSummary or djSummary. */
	let { performer, children }: { performer: PerformerSummary; children?: Snippet } = $props();

	const genres = $derived(performer.genres.slice(0, 3));
</script>

<!-- The name's link stretches over the card; `children` sits above it so its own links still work. -->
<article class="card relative flex h-full flex-col gap-3 p-5 transition-colors outline-link outline-offset-2 hover:border-line-strong has-[a:focus-visible]:outline-2">
	<div class="flex items-center gap-3">
		<Avatar src={performer.avatarUrl} name={performer.name} size="lg" />
		<div class="min-w-0 flex-1">
			<h3 class="line-clamp-2 text-emphasis font-semibold break-words">
				<a href={performer.href} class="text-fg no-underline after:absolute after:inset-0 after:rounded-xl focus-visible:outline-none">{performer.name}</a>
			</h3>
			<p class="text-label text-fg-muted">{[performer.kind, performer.place].filter(Boolean).join(' · ')}</p>
		</div>
		{#if performer.distanceKm !== null}<span class="self-start font-mono text-caption whitespace-nowrap text-fg-muted">{formatDistance(performer.distanceKm)}</span>{/if}
	</div>
	{#if performer.unclaimed}
		<span class="pill self-start bg-well text-fg-muted" title="Nobody has claimed this project yet">Unclaimed</span>
	{/if}
	{#if performer.tagline}<p class="line-clamp-2 text-small text-fg-soft">{performer.tagline}</p>{/if}
	<p class="mt-auto flex flex-wrap justify-between gap-x-3 gap-y-1 font-mono text-caption text-fg-muted">
		<span class="flex flex-wrap gap-x-1.5">
			{#each genres as genre, index (genre)}<span class="whitespace-nowrap">{index < genres.length - 1 ? `${genre} ·` : genre}</span>{/each}
			{#if performer.genres.length > 3}<span class="text-fg-subtle">+{performer.genres.length - 3}</span>{/if}
		</span>
		{#if performer.fee}<span>{performer.fee}</span>{/if}
	</p>
	{#if children}<div class="relative z-10 border-t border-line pt-3">{@render children()}</div>{/if}
</article>
