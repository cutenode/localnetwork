<script lang="ts">
	import { ArrowRight, LogOut, Plus } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import ProfileSetupCard from '#lib/components/ProfileSetupCard.svelte';
	import RequestList from '#lib/components/RequestList.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { bookingGroup } from '#lib/bookings.ts';
	import { accountLabel, formatShowDate, placeLabel, pluralize, timeAgo } from '#lib/format.ts';
	import { projectSummary } from '#lib/performers.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const viewer = $derived(data.viewer?.did ?? '');

	const overview = $derived(Promise.all([data.projects, data.books]));
</script>

<svelte:head><title>Dashboard · Local Network</title></svelte:head>

<!-- A card with a heading and a link to the full page; `count` notes what's waiting there. -->
{#snippet panel(id: string, title: string, href: string, link: string, body: Snippet, count?: string)}
	<Card as="section" padding="none">
		<div class="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
			<h2 {id} class="m-0 flex flex-wrap items-baseline gap-x-2 text-emphasis font-semibold">{title}{#if count}{' '}<span class="text-small font-medium text-warning">{count}</span>{/if}</h2>
			<a {href} class="inline-flex shrink-0 items-center gap-1 text-small text-fg-muted no-underline hover:text-fg">{link}<ArrowRight class="size-3.5" /></a>
		</div>
		{@render body()}
	</Card>
{/snippet}

{#snippet threads(managing: boolean)}
	{#snippet body()}
		{#if data.threads.length}
			<ul class="m-0 list-none divide-y divide-line p-0">
				{#each data.threads as thread (thread.space)}
					<li class="flex items-center justify-between gap-3 px-5 py-3.5">
						<div class="flex min-w-0 flex-col gap-0.5">
							<a class="truncate text-fg no-underline hover:text-link" href="/dashboard/bookings/{thread.skey}">{thread.title ?? 'Booking'} · {thread.project?.name}</a>
							<p class="m-0 text-caption text-fg-subtle">
								{#if bookingGroup(thread, viewer) === 'reply'}<span class="font-medium text-warning">Needs your reply</span> · {/if}{timeAgo(thread.lastActivityAt)}
							</p>
						</div>
						<StatusPill status={thread.status} />
					</li>
				{/each}
			</ul>
		{:else if managing}
			<EmptyState framed={false} title="No booking threads yet" body="When a promoter sends an inquiry to one of your projects, the thread starts here." />
		{:else}
			<EmptyState framed={false} title="No booking threads yet" body="Find a DJ and send an inquiry with your show details.">
				<Button size="sm" href="/projects">Find DJs</Button>
			</EmptyState>
		{/if}
	{/snippet}
	{@render panel('threads', 'Booking threads', '/dashboard/bookings', 'All bookings', body, data.waiting ? `${data.waiting} waiting on you` : undefined)}
{/snippet}

{#snippet submissions()}
	{#snippet body()}<RequestList requests={data.requests} />{/snippet}
	{@render panel('submissions', 'Your submissions', '/dashboard/requests', 'All submissions', body)}
{/snippet}

<div class="flex flex-col gap-10">
	<header class="flex flex-wrap items-end justify-between gap-x-10 gap-y-6">
		<div class="flex min-w-0 flex-col gap-3">
			<Eyebrow>Overview</Eyebrow>
			<h1 class="m-0 text-title font-semibold break-words">{accountLabel(data.viewer)}</h1>
		</div>
		<form method="POST" action="/logout" class="flex"><button class="btn-ghost"><LogOut class="size-4" />Sign out</button></form>
	</header>

	{#if !data.viewer?.spacesCapable}
		<p class="notice notice-info m-0">Your account's server can't keep private data for Local Network yet, so Local Network keeps your bookings and submissions for you (<a href="/dashboard/data">see what it keeps</a>) and the planner is off. Once it can, sign out and back in.</p>
	{/if}

	{#if data.firstRun}<ProfileSetupCard name={data.firstRun.name} hasAvatar={data.firstRun.hasAvatar} />{/if}

	{#await overview}
		<div role="status" aria-busy="true" aria-label="Loading your overview" class="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
			<div class="h-40 animate-pulse rounded-xl bg-surface"></div>
		</div>
	{:then [projects, books]}
		{#if !books}
			<section aria-labelledby="projects" class="flex flex-col gap-5">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<h2 id="projects" class="m-0 text-heading font-semibold">Projects you manage</h2>
					<Button variant="secondary" size="sm" href="/dashboard/projects/new"><Plus class="size-4" />List a project</Button>
				</div>
				{#if projects === null}
					<EmptyState title="Projects are offline" body="The directory isn't answering. Try again in a minute." />
				{:else if projects.length === 0}
					<EmptyState title="You don't manage any projects yet" body="List your own project, or find one an admin already added and claim it.">
						<Button variant="secondary" href="/projects">Find yours</Button>
					</EmptyState>
				{:else}
					<div class="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
						{#each projects as project (project.uri)}
							<PerformerCard performer={projectSummary(project)}>
								<div class="flex gap-2">
									<Button variant="secondary" size="sm" href="/dashboard/projects/{project.slug}" aria-label="Manage {project.profile.name}">Manage</Button>
									<Button variant="ghost" size="sm" href="/projects/{project.slug}" aria-label="View {project.profile.name}">View</Button>
								</div>
							</PerformerCard>
						{/each}
					</div>
				{/if}
			</section>
			<div class="grid gap-6 xl:grid-cols-2">
				{@render threads(true)}
				{@render submissions()}
			</div>
		{:else}
			{@render threads(false)}
			{#await data.planner}
				<div role="status" aria-busy="true" aria-label="Loading your planner" class="grid gap-6 xl:grid-cols-2">
					<div class="h-40 animate-pulse rounded-xl bg-surface"></div>
					<div class="h-40 animate-pulse rounded-xl bg-surface"></div>
				</div>
			{:then planner}
				{#if planner?.available}
					<div class="grid gap-6 xl:grid-cols-2">
						{#snippet shows()}
							{#if planner.shows.length}
								<ul class="m-0 list-none divide-y divide-line p-0">
									{#each planner.shows.slice(0, 5) as show (show.rkey)}
										<li class="flex flex-col gap-0.5 px-5 py-3.5">
											<a class="truncate text-fg no-underline hover:text-link" href="/dashboard/planner/shows/{show.rkey}">{show.details.title}</a>
											<p class="m-0 text-caption text-fg-subtle">{[show.details.date ? formatShowDate(show.details.date) : 'No date yet', show.details.venue, pluralize(show.lineup.length, 'DJ')].filter(Boolean).join(' · ')}</p>
										</li>
									{/each}
								</ul>
							{:else}
								<EmptyState framed={false} title="No shows planned" body="Plan a night and build its lineup here before you send inquiries.">
									<Button variant="secondary" size="sm" href="/dashboard/planner/shows/new">Plan a show</Button>
								</EmptyState>
							{/if}
						{/snippet}
						{#snippet saved()}
							{@const projects = planner.saves.flatMap((save) => planner.projects[save.project] ?? [])}
							{#if projects.length}
								<ul class="m-0 list-none divide-y divide-line p-0">
									{#each projects.slice(0, 5) as project (project.uri)}
										<li class="flex flex-col gap-0.5 px-5 py-3.5">
											<a class="truncate text-fg no-underline hover:text-link" href="/projects/{project.slug}">{project.profile.name}</a>
											<p class="m-0 text-caption text-fg-subtle">{placeLabel(project.profile)}</p>
										</li>
									{/each}
								</ul>
							{:else}
								<EmptyState framed={false} title="Nothing saved" body="Save DJs from their profiles to keep a shortlist here.">
									<Button variant="secondary" size="sm" href="/projects">Find DJs</Button>
								</EmptyState>
							{/if}
						{/snippet}
						{@render panel('planned', 'Planned shows', '/dashboard/planner', 'Planner', shows)}
						{@render panel('saved', 'Saved projects', '/dashboard/planner#saved', 'All saved', saved)}
					</div>
				{:else if planner && 'problem' in planner}
					<EmptyState title="Your planner didn't load" body="Try again in a minute." />
				{/if}
			{/await}
			{#if data.requests.length}{@render submissions()}{/if}
			<p class="m-0 text-small text-fg-muted">DJ yourself? <a class="link" href="/dashboard/projects/new">List a project</a></p>
		{/if}
	{/await}
</div>
