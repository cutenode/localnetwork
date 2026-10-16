<script lang="ts">
	import { formatDistance } from '#lib/geo.ts';
	import { feeLabel, kindLabel } from '#lib/format.ts';
	import type { ProjectView } from '#lib/types.ts';
	import Avatar from './Avatar.svelte';
	import type { Snippet } from 'svelte';

	let { project, children }: { project: ProjectView; children?: Snippet } = $props();

	const profile = $derived(project.profile);

	const distance = $derived(project.distanceKm ?? null);

	const genres = $derived(profile.genres.slice(0, 3));
</script>

<!-- The name's link stretches over the card; `children` sits above it so its own links still work. -->
<article class="card relative flex h-full flex-col gap-3 p-5 transition-colors outline-link outline-offset-2 hover:border-line-strong has-[a:focus-visible]:outline-2">
	<div class="flex items-center gap-3">
		<Avatar src={project.avatarUrl} name={profile.name} size="lg" />
		<div class="min-w-0 flex-1">
			<h3 class="line-clamp-2 text-emphasis font-semibold break-words">
				<a href="/projects/{project.slug}" class="text-fg no-underline after:absolute after:inset-0 after:rounded-xl focus-visible:outline-none">{profile.name}</a>
			</h3>
			<p class="text-label text-fg-muted">{kindLabel(profile.kind)} · {profile.location.city}, {profile.location.country}</p>
		</div>
		{#if distance !== null}<span class="self-start font-mono text-caption whitespace-nowrap text-fg-muted">{formatDistance(distance)}</span>{/if}
	</div>
	{#if project.managers.length === 0}
		<span class="pill self-start bg-well text-fg-muted" title="Nobody has claimed this project yet">Unclaimed</span>
	{/if}
	{#if profile.tagline}<p class="line-clamp-2 text-small text-fg-soft">{profile.tagline}</p>{/if}
	<p class="mt-auto flex flex-wrap justify-between gap-x-3 gap-y-1 font-mono text-caption text-fg-muted">
		<span class="flex flex-wrap gap-x-1.5">
			{#each genres as genre, index (genre)}<span class="whitespace-nowrap">{index < genres.length - 1 ? `${genre} ·` : genre}</span>{/each}
			{#if profile.genres.length > 3}<span class="text-fg-subtle">+{profile.genres.length - 3}</span>{/if}
		</span>
		{#if profile.fee}<span>{feeLabel(profile)}</span>{/if}
	</p>
	{#if children}<div class="relative z-10 border-t border-line pt-3">{@render children()}</div>{/if}
</article>
