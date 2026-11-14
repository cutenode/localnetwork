<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatShowDate, pluralize } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tools = [
		{ href: '/match', title: 'Match a show', body: 'Describe the night and get a shortlist, with the reasons each DJ made it.' },
		{ href: '/for-promoters', title: "What's new for promoters", body: 'Everything you can do here, and what is coming next.' }
	];

	const row = 'relative flex items-center gap-3 px-4 py-3 transition-colors hover:bg-raised sm:px-5';
</script>

<svelte:head><title>Book DJs · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-10">
	<SectionHeader level={1} title="Book DJs" lede="Search the directory, match a show, and keep your inquiries and lineups in one place." />

	<div class="flex flex-col gap-4">
		<form action="/projects" method="GET" role="search" aria-label="Find DJs" class="grid w-full items-end gap-3 rounded-2xl border border-line bg-surface p-4 sm:grid-cols-[1fr_1fr_auto] sm:p-5">
			<div>
				<label for="promoter-q" class="label">Sound or name</label>
				<input id="promoter-q" name="q" class="input" placeholder="disco, jungle, a name" />
			</div>
			<div>
				<label for="promoter-near" class="label">Near</label>
				<NearInput id="promoter-near" geocoder={data.geocoder} />
			</div>
			<button class="btn-primary">Find DJs</button>
		</form>
		<ul class="m-0 grid list-none gap-x-8 p-0 sm:grid-cols-2">
			{#each tools as tool (tool.href)}
				<li class="flex flex-col gap-1 border-t border-line py-4">
					<a class="link font-semibold" href={tool.href}>{tool.title}</a>
					<span class="text-small text-fg-muted">{tool.body}</span>
				</li>
			{/each}
		</ul>
	</div>

	<section aria-labelledby="inquiries-title" class="flex flex-col gap-4">
		<div class="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
			<h2 id="inquiries-title" class="m-0 text-heading font-semibold">Your inquiries</h2>
			{#if data.inquiryCount}<a class="link text-small" href="/dashboard/bookings">All bookings</a>{/if}
		</div>
		{#if data.inquiries.length}
			<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0">
				{#each data.inquiries as thread (thread.space)}
					<li class={row}>
						<div class="flex min-w-0 flex-1 flex-col gap-0.5">
							<a href="/dashboard/bookings/{thread.skey}" class="truncate text-small font-medium text-fg no-underline after:absolute after:inset-0">{thread.title ?? 'Booking inquiry'}</a>
							<p class="m-0 text-label text-fg-muted">{thread.project?.name ?? 'Unknown project'}{thread.showDate ? ` · ${formatShowDate(thread.showDate)}` : ''}</p>
						</div>
						<StatusPill status={thread.status} />
					</li>
				{/each}
			</ul>
			{#if data.inquiryCount > data.inquiries.length}<p class="m-0 text-small text-fg-muted">{pluralize(data.inquiryCount - data.inquiries.length, 'more inquiry', 'more inquiries')} in <a class="link" href="/dashboard/bookings">Bookings</a>.</p>{/if}
		{:else}
			<EmptyState title="No inquiries yet" body="Find a DJ and send an inquiry with your show details. Each one is a private thread." />
		{/if}
	</section>

	<section aria-labelledby="planned-title" class="flex flex-col gap-4">
		<div class="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
			<h2 id="planned-title" class="m-0 text-heading font-semibold">Planned shows</h2>
			<a class="link text-small" href="/dashboard/planner">Open your planner</a>
		</div>
		{#await data.planner}
			<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0" aria-busy="true">
				{#each { length: 2 }, index (index)}
					<li class="flex flex-col gap-2 px-4 py-3 sm:px-5"><span class="h-4 w-2/5 animate-pulse rounded-md bg-raised"></span><span class="h-3 w-3/5 animate-pulse rounded-md bg-raised"></span></li>
				{/each}
			</ul>
		{:then planner}
			{#if !planner.available}
				<EmptyState title="Planner unavailable" body={planner.problem ?? "Your PDS doesn't support atproto spaces yet."} />
			{:else if planner.shows.length === 0}
				<EmptyState title="No shows planned" body="Plan a night and build a lineup, or use the matcher to start one.">
					<Button href="/match">Open the matcher</Button>
				</EmptyState>
			{:else}
				<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0">
					{#each planner.shows as show (show.rkey)}
						<li class={row}>
							<div class="flex min-w-0 flex-1 flex-col gap-0.5">
								<a href="/dashboard/planner/shows/{show.rkey}" class="truncate text-small font-medium text-fg no-underline after:absolute after:inset-0">{show.details.title}</a>
								<p class="m-0 text-label text-fg-muted">{[show.details.date ? formatShowDate(show.details.date) : 'No date yet', show.details.venue, pluralize(show.lineup.length, 'DJ')].filter(Boolean).join(' · ')}</p>
							</div>
						</li>
					{/each}
				</ul>
				{#if planner.total > planner.shows.length}<p class="m-0 text-small text-fg-muted">{pluralize(planner.total - planner.shows.length, 'more show')} in <a class="link" href="/dashboard/planner">your planner</a>.</p>{/if}
			{/if}
		{/await}
	</section>
</div>
