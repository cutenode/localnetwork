<script lang="ts">
	import { ArrowRight, CalendarHeart, ClipboardList, Disc3, History, NotebookPen, Plus } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { accountLabel, timeAgo } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const kindLabel = { create: 'New project', claim: 'Claim', update: 'Changes' } as const;
</script>

<svelte:head><title>My account · Local Network</title></svelte:head>

<div class="container-page py-10">
	<div class="flex flex-wrap items-end justify-between gap-4">
		<div>
			<p class="eyebrow">Dashboard</p>
			<h1 class="mt-1 text-4xl">{accountLabel(data.viewer)}</h1>
			<p class="muted mono mt-1">{data.viewer?.did}</p>
		</div>
		<div class="flex flex-wrap gap-2">
			<a href="/projects/new" class="btn-accent"><Plus class="size-4" />List a project</a>
			<a href="/planner" class="btn-secondary"><NotebookPen class="size-4" />Planner</a>
			<a href="/me/activity" class="btn-secondary"><History class="size-4" />My activity</a>
		</div>
	</div>

	{#if !data.viewer?.spacesCapable}
		<div class="mt-6 notice notice-info">
			Your PDS didn't grant atproto spaces access, so requests and bookings are filed for you by Local Network's root account and your private planner is unavailable. Everything you do is still audited. If your PDS adds spaces support, sign out and back in.
		</div>
	{/if}

	<section class="mt-10">
		<h2 class="section-title mb-4 flex items-center gap-2"><Disc3 class="size-5" />Projects you manage</h2>
		{#if data.projects.length}
			<div class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
				{#each data.projects as project (project.uri)}
					<ProjectCard {project}>
						<div class="flex gap-2">
							<a class="btn-secondary btn-sm" href="/projects/{project.slug}/edit">Propose changes</a>
							<a class="btn-ghost btn-sm" href="/projects/{project.slug}">View</a>
						</div>
					</ProjectCard>
				{/each}
			</div>
		{:else}
			<EmptyState title="You don't manage any projects yet" body="List your own project, or find one an admin already added and claim it.">
				<div class="flex gap-2"><a href="/projects/new" class="btn-primary">List a project</a><a href="/projects" class="btn-secondary">Find yours</a></div>
			</EmptyState>
		{/if}
	</section>

	<div class="mt-10 grid gap-6 lg:grid-cols-2">
		<section class="card card-pad">
			<h2 class="section-title mb-4 flex items-center gap-2"><ClipboardList class="size-5" />Your requests</h2>
			{#if data.requests.length}
				<ul class="divide-y-2 divide-dashed divide-ansi-blue">
					{#each data.requests as request (request.uri)}
						<li class="flex items-center justify-between gap-3 py-3">
							<div class="min-w-0">
								<a class="menu-link" href="/me/requests/{request.rkey}">{kindLabel[request.kind]}: {request.project?.name ?? request.record.profile?.name ?? 'Project'}</a>
								<p class="muted text-xs">{timeAgo(request.createdAt)}</p>
							</div>
							<StatusPill status={request.status} />
						</li>
					{/each}
				</ul>
			{:else}
				<p class="muted text-sm">No requests yet.</p>
			{/if}
		</section>
		<section class="card card-pad">
			<div class="mb-4 flex items-center justify-between">
				<h2 class="section-title flex items-center gap-2"><CalendarHeart class="size-5" />Recent bookings</h2>
				<a href="/bookings" class="link text-sm">All <ArrowRight class="inline size-3" /></a>
			</div>
			{#if data.threads.length}
				<ul class="divide-y-2 divide-dashed divide-ansi-blue">
					{#each data.threads as thread (thread.space)}
						<li class="flex items-center justify-between gap-3 py-3">
							<div class="min-w-0">
								<a class="menu-link" href="/bookings/{thread.skey}">{thread.title ?? 'Booking'} · {thread.project?.name}</a>
								<p class="muted text-xs">{timeAgo(thread.lastActivityAt)}</p>
							</div>
							<StatusPill status={thread.status} />
						</li>
					{/each}
				</ul>
			{:else}
				<p class="muted text-sm">No booking threads yet.</p>
			{/if}
		</section>
	</div>
</div>
