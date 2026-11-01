<script lang="ts">
	import { ArrowRight, LogOut, Plus } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import RequestList from '#lib/components/RequestList.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { accountLabel, timeAgo } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Dashboard · Local Network</title></svelte:head>

<div class="flex flex-col gap-10">
	<header class="flex flex-wrap items-end justify-between gap-x-10 gap-y-6">
		<div class="flex min-w-0 flex-col gap-3">
			<Eyebrow>Overview</Eyebrow>
			<h1 class="m-0 text-title font-semibold break-words">{accountLabel(data.viewer)}</h1>
			<p class="m-0 font-mono text-caption break-all text-fg-subtle">{data.viewer?.did}</p>
		</div>
		<div class="grid w-full grid-cols-2 gap-2 sm:flex sm:w-auto sm:flex-wrap">
			<Button href="/dashboard/projects/new"><Plus class="size-4" />List a project</Button>
			<form method="POST" action="/logout" class="flex"><button class="btn-ghost w-full"><LogOut class="size-4" />Sign out</button></form>
		</div>
	</header>

	{#if !data.viewer?.spacesCapable}
		<p class="notice notice-info m-0">
			Your PDS didn't grant atproto spaces access, so Local Network's root account files your requests and bookings for you, and the private planner is unavailable. Everything you do is still audited. If your PDS adds spaces support, sign out and back in.
		</p>
	{/if}

	<section aria-labelledby="projects" class="flex flex-col gap-5">
		<h2 id="projects" class="m-0 text-heading font-semibold">Projects you manage</h2>
		{#await data.projects}
			<div role="status" aria-busy="true" aria-label="Loading your projects" class="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
				<div class="h-40 animate-pulse rounded-xl bg-surface"></div>
			</div>
		{:then projects}
			{#if projects === null}
				<EmptyState title="Projects are offline" body="The directory isn't answering. Try again in a minute." />
			{:else if projects.length}
				<div class="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
					{#each projects as project (project.uri)}
						<ProjectCard {project}>
							<div class="flex gap-2">
								<Button variant="secondary" size="sm" href="/dashboard/projects/{project.slug}" aria-label="Manage {project.profile.name}">Manage</Button>
								<Button variant="ghost" size="sm" href="/projects/{project.slug}" aria-label="View {project.profile.name}">View</Button>
							</div>
						</ProjectCard>
					{/each}
				</div>
			{:else}
				<EmptyState title="You don't manage any projects yet" body="List your own project, or find one an admin already added and claim it.">
					<div class="flex flex-wrap justify-center gap-2"><Button href="/dashboard/projects/new">List a project</Button><Button variant="secondary" href="/projects">Find yours</Button></div>
				</EmptyState>
			{/if}
		{/await}
	</section>

	<div class="grid gap-6 xl:grid-cols-2">
		<Card as="section" padding="none">
			<div class="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
				<h2 class="m-0 text-emphasis font-semibold">Your requests</h2>
				<a href="/dashboard/requests" class="inline-flex items-center gap-1 text-small text-fg-muted no-underline hover:text-fg">All requests<ArrowRight class="size-3.5" /></a>
			</div>
			<RequestList requests={data.requests} />
		</Card>
		<Card as="section" padding="none">
			<div class="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
				<h2 class="m-0 text-emphasis font-semibold">Recent bookings</h2>
				<a href="/dashboard/bookings" class="inline-flex items-center gap-1 text-small text-fg-muted no-underline hover:text-fg">All bookings<ArrowRight class="size-3.5" /></a>
			</div>
			{#if data.threads.length}
				<ul class="m-0 list-none divide-y divide-line p-0">
					{#each data.threads as thread (thread.space)}
						<li class="flex items-center justify-between gap-3 px-5 py-3.5">
							<div class="flex min-w-0 flex-col gap-0.5">
								<a class="truncate text-fg no-underline hover:text-link" href="/dashboard/bookings/{thread.skey}">{thread.title ?? 'Booking'} · {thread.project?.name}</a>
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
