<script lang="ts">
	import { enhance } from '$app/forms';
	import { CalendarDays, Lock, Plus, Trash2 } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import { formatShowDate } from '#lib/format.ts';
	import type { ProjectView } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const projects: Record<string, ProjectView> = $derived(data.projects);
</script>

<svelte:head><title>Planner · Local Network</title></svelte:head>

<div class="container-page py-10">
	<p class="eyebrow flex items-center gap-1"><Lock class="size-3" />Private</p>
	<h1 class="mt-1 text-4xl">Your planner</h1>
	<p class="muted mt-2 max-w-2xl">Saved projects and shows you're booking. This lives in a personal atproto space on your own PDS; Local Network keeps no copy.</p>
	<div class="mt-6"><FormMessage {form} /></div>

	{#if !data.available}
		<div class="mt-8"><EmptyState title="Planner unavailable" body={'problem' in data && data.problem ? data.problem : "Your PDS doesn't support atproto spaces yet."} /></div>
	{:else}
		<section class="mt-10">
			<div class="mb-4 flex items-center justify-between">
				<h2 class="section-title flex items-center gap-2"><CalendarDays class="size-5" />Shows</h2>
				<a href="/planner/shows/new" class="btn-secondary btn-sm"><Plus class="size-3" />New show</a>
			</div>
			{#if data.shows.length === 0}
				<EmptyState title="No shows planned" body="Plan a night and build a lineup, or use the matcher to start one."><a class="btn-primary" href="/match">Open the matcher</a></EmptyState>
			{:else}
				<div class="grid gap-4 md:grid-cols-2">
					{#each data.shows as show (show.rkey)}
						<article class="card card-pad space-y-3">
							<div class="flex items-start justify-between gap-3">
								<div>
									<a class="menu-link text-2xl leading-none uppercase" href="/planner/shows/{show.rkey}">{show.details.title}</a>
									<p class="muted text-sm">{show.details.date ? formatShowDate(show.details.date) : 'No date yet'}{show.details.venue ? ` · ${show.details.venue}` : ''}</p>
								</div>
								<form method="POST" action="?/deleteShow" use:enhance={({ cancel }) => { if (!confirm('Delete this show?')) cancel(); }}>
									<input type="hidden" name="rkey" value={show.rkey} />
									<button class="btn-ghost px-2" aria-label="Delete show"><Trash2 class="size-4" /></button>
								</form>
							</div>
							<ul class="flex flex-wrap gap-1.5">
								{#each show.lineup as slot (slot.project)}
									<li class="chip">{projects[slot.project]?.profile.name ?? 'Removed project'}{slot.status ? ` · ${slot.status}` : ''}</li>
								{:else}
									<li class="muted text-sm">No lineup yet</li>
								{/each}
							</ul>
						</article>
					{/each}
				</div>
			{/if}
		</section>

		<section class="mt-12">
			<h2 class="section-title mb-4">Saved projects</h2>
			{#if data.saves.length === 0}
				<EmptyState title="Nothing saved" body="Tap Save on any project profile to keep it here." />
			{:else}
				<div class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
					{#each data.saves as save (save.rkey)}
						{#if projects[save.project]}
							<ProjectCard project={projects[save.project]}>
								<form method="POST" action="?/unsave" use:enhance>
									<input type="hidden" name="project" value={save.project} />
									<button class="btn-ghost btn-sm">Remove</button>
								</form>
							</ProjectCard>
						{/if}
					{/each}
				</div>
			{/if}
		</section>
	{/if}
</div>
