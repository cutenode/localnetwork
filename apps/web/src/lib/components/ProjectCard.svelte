<script lang="ts">
	import { formatDistance } from '#lib/geo.ts';
	import { feeLabel, kindLabel } from '#lib/format.ts';
	import type { ProjectView } from '#lib/types.ts';
	import Avatar from './Avatar.svelte';
	import type { Snippet } from 'svelte';

	let { project, children }: { project: ProjectView; children?: Snippet } = $props();

	const profile = $derived(project.profile);
</script>

<article class="card group relative flex h-full flex-col gap-2 px-4 pt-3 pb-4 focus-within:border-ansi-yellow hover:border-ansi-bright-cyan">
	<div class="flex items-start gap-3">
		<Avatar src={project.avatarUrl} name={profile.name} />
		<div class="min-w-0 flex-1">
			<h3 class="line-clamp-2 text-2xl leading-none break-words">
				<a href="/projects/{project.slug}" class="text-ansi-bright-cyan uppercase after:absolute after:inset-0 group-hover:underline focus:outline-none">{profile.name}</a>
			</h3>
			<p class="mt-1 flex flex-wrap gap-x-2 text-ansi-bright-green">
				<span>{profile.location.city}, {profile.location.country}</span>
				{#if project.distanceKm !== undefined && project.distanceKm !== null}
					<span class="whitespace-nowrap text-ansi-white">{formatDistance(project.distanceKm)}</span>
				{/if}
			</p>
			<p class="text-ansi-yellow">{[kindLabel(profile.kind), profile.fee && feeLabel(profile)].filter(Boolean).join(' · ')}</p>
		</div>
	</div>
	{#if project.managers.length === 0}
		<p class="text-ansi-bright-red uppercase" title="Nobody has claimed this project yet">Unclaimed</p>
	{/if}
	{#if profile.tagline}<p class="line-clamp-2 text-ansi-white">{profile.tagline}</p>{/if}
	<p class="mt-auto text-ansi-bright-magenta">
		{profile.genres.slice(0, 5).join(' / ')}{#if profile.genres.length > 5}<span class="text-ansi-cyan"> +{profile.genres.length - 5}</span>{/if}
	</p>
	{#if children}<div class="relative z-10 border-t-2 border-dashed border-ansi-blue pt-2">{@render children()}</div>{/if}
</article>
