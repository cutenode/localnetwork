<script lang="ts">
	import { ArrowRight, History, LogOut, NotebookPen, Plus } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { accountLabel, timeAgo } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const kindLabel = { create: 'New project', claim: 'Claim', update: 'Changes' } as const;
</script>

<svelte:head><title>My account · Local Network</title></svelte:head>

<div class="container-page flex flex-col gap-12 py-10 sm:py-14">
	<header class="flex flex-wrap items-end justify-between gap-x-10 gap-y-6">
		<div class="flex min-w-0 flex-col gap-3">
			<Eyebrow>Dashboard</Eyebrow>
			<h1 class="m-0 text-title font-semibold break-words">{accountLabel(data.viewer)}</h1>
			<p class="m-0 font-mono text-caption break-all text-fg-subtle">{data.viewer?.did}</p>
		</div>
		<div class="grid w-full grid-cols-2 gap-2 sm:flex sm:w-auto sm:flex-wrap">
			<Button href="/projects/new" class="col-span-2"><Plus class="size-4" />List a project</Button>
			<Button variant="secondary" href="/planner"><NotebookPen class="size-4" />Planner</Button>
			<Button variant="secondary" href="/me/activity"><History class="size-4" />My activity</Button>
			<form method="POST" action="/logout" class="col-span-2 flex"><button class="btn-ghost w-full"><LogOut class="size-4" />Sign out</button></form>
		</div>
	</header>

	{#if !data.viewer?.spacesCapable}
		<p class="notice notice-info m-0">
			Your PDS didn't grant atproto spaces access, so Local Network's root account files your requests and bookings for you, and the private planner is unavailable. Everything you do is still audited. If your PDS adds spaces support, sign out and back in.
		</p>
	{/if}

	<section aria-labelledby="projects" class="flex flex-col gap-5">
		<h2 id="projects" class="m-0 text-heading font-semibold">Projects you manage</h2>
		{#if data.projects.length}
			<div class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
				{#each data.projects as project (project.uri)}
					<ProjectCard {project}>
						<div class="flex gap-2">
							<Button variant="secondary" size="sm" href="/projects/{project.slug}/edit">Propose changes</Button>
							<Button variant="ghost" size="sm" href="/projects/{project.slug}">View</Button>
						</div>
					</ProjectCard>
				{/each}
			</div>
		{:else}
			<EmptyState title="You don't manage any projects yet" body="List your own project, or find one an admin already added and claim it.">
				<div class="flex flex-wrap justify-center gap-2"><Button href="/projects/new">List a project</Button><Button variant="secondary" href="/projects">Find yours</Button></div>
			</EmptyState>
		{/if}
	</section>

	<div class="grid gap-6 lg:grid-cols-2">
		<Card as="section" padding="none">
			<h2 class="m-0 border-b border-line px-5 py-4 text-emphasis font-semibold">Your requests</h2>
			{#if data.requests.length}
				<ul class="m-0 list-none divide-y divide-line p-0">
					{#each data.requests as request (request.uri)}
						<li class="flex items-center justify-between gap-3 px-5 py-3.5">
							<div class="flex min-w-0 flex-col gap-0.5">
								<a class="truncate text-fg no-underline hover:text-link" href="/me/requests/{request.rkey}">{kindLabel[request.kind]}: {request.project?.name ?? request.record.profile?.name ?? 'Project'}</a>
								<p class="m-0 text-caption text-fg-subtle">{timeAgo(request.createdAt)}</p>
							</div>
							<StatusPill status={request.status} />
						</li>
					{/each}
				</ul>
			{:else}
				<p class="m-0 px-5 py-6 text-small text-fg-muted">No requests yet.</p>
			{/if}
		</Card>
		<Card as="section" padding="none">
			<div class="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
				<h2 class="m-0 text-emphasis font-semibold">Recent bookings</h2>
				<a href="/bookings" class="inline-flex items-center gap-1 text-small text-fg-muted no-underline hover:text-fg">All bookings<ArrowRight class="size-3.5" /></a>
			</div>
			{#if data.threads.length}
				<ul class="m-0 list-none divide-y divide-line p-0">
					{#each data.threads as thread (thread.space)}
						<li class="flex items-center justify-between gap-3 px-5 py-3.5">
							<div class="flex min-w-0 flex-col gap-0.5">
								<a class="truncate text-fg no-underline hover:text-link" href="/bookings/{thread.skey}">{thread.title ?? 'Booking'} · {thread.project?.name}</a>
								<p class="m-0 text-caption text-fg-subtle">{timeAgo(thread.lastActivityAt)}</p>
							</div>
							<StatusPill status={thread.status} />
						</li>
					{/each}
				</ul>
			{:else}
				<p class="m-0 px-5 py-6 text-small text-fg-muted">No booking threads yet.</p>
			{/if}
		</Card>
	</div>
</div>
