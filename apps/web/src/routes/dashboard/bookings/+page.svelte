<script lang="ts">
	import { page } from '$app/state';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { BOOKING_GROUPS, bookingGroup } from '#lib/bookings.ts';
	import { accountLabel, formatShowDate, timeAgo } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const viewer = $derived(data.viewer?.did ?? '');

	// Who the threads are with, for the filter, keyed by an act's slug or a DJ's `@handle`; it shows once there's more
	// than one.
	const filterKey = (project: NonNullable<(typeof data.threads)[number]['project']>) => project.slug ?? project.href.slice(1);

	const projects = $derived([...new Map(data.threads.flatMap((thread) => (thread.project ? [[filterKey(thread.project), thread.project.name] as const] : []))).entries()]);

	const filter = $derived(page.url.searchParams.get('project'));

	const groups = $derived(
		BOOKING_GROUPS.map((group) => ({
			...group,
			threads: data.threads.filter((thread) => (!filter || (thread.project && filterKey(thread.project) === filter)) && bookingGroup(thread, viewer) === group.value)
		})).filter((group) => group.threads.length)
	);

	const chip = 'chip min-h-9 px-3 transition-colors hover:border-fg-subtle aria-[current=page]:border-fg aria-[current=page]:bg-fg aria-[current=page]:font-semibold aria-[current=page]:text-fg-inverse';
</script>

<svelte:head><title>Bookings · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<SectionHeader level={1} eyebrow="Bookings" title="Your booking threads" lede="Each thread is private to the promoter and the act or DJ they asked." />
	{#if data.threads.length === 0}
		{#await Promise.all([data.projects, data.sections]) then [managed, sections]}
			{#if managed?.length || sections?.performer}
				<EmptyState title="No booking threads yet" body="When a promoter sends an inquiry to you or one of your projects, the thread starts here." />
			{:else}
				<EmptyState title="No booking threads yet" body="Find a project and send an inquiry with your show details.">
					<Button href="/projects">Find DJs</Button>
				</EmptyState>
			{/if}
		{/await}
	{:else}
		{#if projects.length > 1}
			<nav aria-label="Filter by project" class="flex flex-wrap gap-2">
				<a href="/dashboard/bookings" aria-current={filter ? undefined : 'page'} class={chip}>All</a>
				{#each projects as [slug, name] (slug)}<a href="/dashboard/bookings?project={slug}" aria-current={filter === slug ? 'page' : undefined} class={chip}>{name}</a>{/each}
			</nav>
		{/if}
		{#each groups as group (group.value)}
			<section aria-labelledby="group-{group.value}" class="flex flex-col gap-3">
				<h2 id="group-{group.value}" class="m-0 flex items-baseline gap-2 text-emphasis font-semibold">{group.label}<span class="font-mono text-caption font-normal text-fg-subtle">{group.threads.length}</span></h2>
				<ul class="m-0 flex list-none flex-col gap-3 p-0">
					{#each group.threads as thread (thread.space)}
						{@const role = thread.participants.find((p) => p.did === viewer)?.role}
						<li>
							<Card href="/dashboard/bookings/{thread.skey}" padding="sm" class="flex items-start gap-3.5 sm:items-center sm:p-5">
								<Monogram name={thread.project?.name ?? 'Booking'} />
								<div class="flex min-w-0 flex-1 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between sm:gap-4">
									<div class="flex min-w-0 flex-col gap-0.5">
										<p class="m-0 truncate text-emphasis font-semibold">{thread.title ?? 'Booking inquiry'}</p>
										<p class="m-0 text-small text-fg-muted">{thread.project?.name ?? 'Unknown project'}{thread.showDate ? ` · ${formatShowDate(thread.showDate)}` : ''}</p>
										<p class="m-0 text-caption text-fg-subtle">{role === 'promoter' ? 'You asked' : `From ${accountLabel(thread.promoter)}`} · active {timeAgo(thread.lastActivityAt)}</p>
									</div>
									<div><StatusPill status={thread.status} /></div>
								</div>
							</Card>
						</li>
					{/each}
				</ul>
			</section>
		{:else}
			<EmptyState title="No threads with this project" body="Pick another project, or show them all.">
				<Button variant="secondary" href="/dashboard/bookings">Show all</Button>
			</EmptyState>
		{/each}
	{/if}
</div>
