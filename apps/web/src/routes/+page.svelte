<script lang="ts">
	import { LockKeyhole } from '@lucide/svelte';
	import HeroHeadline from '#lib/components/HeroHeadline.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import ShowList from '#lib/components/ShowList.svelte';
	import { projectSummary } from '#lib/performers.ts';
	import { pluralize } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Section from '#lib/ui/Section.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// The headline holds still while someone types in the search under it.
	let typing = $state(false);

	/** What promoters get, and whether it needs an account. */
	const perks = [
		{ href: '/match', title: 'Match a show', body: 'Describe the night and get a shortlist, with the reasons each DJ made it.', signIn: false },
		{ href: '/dashboard/bookings', title: 'Private inquiries', body: 'Each booking is its own thread, shared only with the people who answer for the DJ.', signIn: true },
		{ href: '/dashboard/planner', title: 'Planner', body: 'Keep shortlists and build a lineup slot by slot, in a space only you can read.', signIn: true }
	];
</script>

<section aria-labelledby="hero-title" class="border-b border-line bg-canvas bg-grid">
	<div class="container-page flex flex-col items-center gap-8 py-14 sm:gap-10 sm:py-24">
		{#snippet headline()}<HeroHeadline paused={typing} />{/snippet}
		<SectionHeader id="hero-title" level={1} size="display" align="center" title={headline} lede="Promoters find DJs by sound, distance and budget. DJs keep a profile they own, with their booking details kept private." />
		<form action="/projects" method="GET" role="search" aria-label="Find DJs" onfocusin={() => (typing = true)} onfocusout={(event) => (typing = event.currentTarget.contains(event.relatedTarget as Node | null))} class="grid w-full max-w-3xl items-end gap-3 rounded-2xl border border-line bg-surface p-4 sm:grid-cols-[1fr_1fr_auto] sm:p-5">
			<div>
				<label for="hero-q" class="label">Sound or name</label>
				<input id="hero-q" name="q" class="input" placeholder="disco, jungle, a name" />
			</div>
			<div>
				<label for="hero-near" class="label">Near</label>
				<NearInput id="hero-near" geocoder={data.geocoder} />
			</div>
			<button class="btn-primary">Find DJs</button>
		</form>
		<p class="m-0 text-small text-fg-muted">Play out yourself? <a class="link" href="/for-djs">Get listed</a></p>
	</div>
</section>

<Section labelledby="soon-title">
	<div class="grid gap-10 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:gap-14">
		<div class="flex flex-col gap-5">
			<div class="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
				<h2 id="soon-title" class="m-0 text-heading font-semibold">Coming up</h2>
				<a class="link text-small" href="/shows">All shows</a>
			</div>
			{#await data.shows}
				<ShowList shows={undefined} rows={3} empty={{ title: '' }} />
			{:then shows}
				<ShowList shows={shows?.slice(0, 3) ?? null} retryHref="/" empty={{ title: 'No shows announced yet', body: 'DJs announce theirs from their dashboard.' }} />
			{/await}
		</div>
		<div class="flex flex-col gap-5">
			<div class="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
				<h2 id="played-title" class="m-0 text-heading font-semibold">Most played this month</h2>
				<a class="link text-small" href="/charts">Charts</a>
			</div>
			<!-- Five rows from the start, so the list arriving doesn't move what's under it. -->
			<ol aria-labelledby="played-title" class="m-0 flex min-h-[calc(5*3.25rem)] list-none flex-col p-0">
				{#await data.tracks}
					{#each { length: 5 }, row (row)}<li class="flex h-13 items-center border-b border-line last:border-b-0"><span class="h-4 w-2/3 animate-pulse rounded-md bg-raised"></span></li>{/each}
				{:then tracks}
					{#if !tracks?.length}
						<li class="py-3 text-small text-fg-muted">{tracks ? 'Nothing played this month yet.' : "The charts aren't answering. Try again in a minute."}</li>
					{:else}
						{#each tracks as track, index (track.id)}
							<li class="flex h-13 items-center gap-4 border-b border-line last:border-b-0">
								<span class="w-4 font-mono text-caption text-fg-subtle tabular-nums" aria-hidden="true">{index + 1}</span>
								<span class="flex min-w-0 flex-1 flex-col">
									<span class="truncate text-small font-semibold">{track.title}</span>
									{#if track.artist}<span class="truncate text-label text-fg-muted">{track.artist}</span>{/if}
								</span>
								<span class="font-mono text-caption whitespace-nowrap text-fg-muted tabular-nums">{pluralize(track.supports, 'DJ')}</span>
							</li>
						{/each}
					{/if}
				{/await}
			</ol>
		</div>
	</div>
</Section>

<Section labelledby="recent-title">
	<div class="flex flex-col gap-8 sm:gap-12">
		<div class="flex flex-wrap items-end justify-between gap-x-8 gap-y-4">
			<div class="flex flex-col gap-3">
				<SectionHeader id="recent-title" tone="low" title="New on the directory" />
				{#if data.stats?.active}
					<p class="m-0 text-lede text-fg-muted">
						{pluralize(data.stats.active, 'DJ')} in {pluralize(data.stats.cities, 'city', 'cities')}{#if data.stats.unclaimed}, <a class="link" href="/projects?unclaimed=on">{data.stats.unclaimed} up for claiming</a>{/if}.
					</p>
				{/if}
			</div>
			{#if data.recent.length}<Button variant="secondary" href="/projects?sort=recent">See all DJs</Button>{/if}
		</div>
		{#if !data.stats}
			<Card tone="outline" padding="lg" radius="2xl">
				<p class="m-0 font-semibold">The directory isn't answering right now.</p>
				<p class="m-0 mt-1 text-fg-muted">Try again in a minute.</p>
			</Card>
		{:else if data.recent.length === 0}
			<Card tone="outline" padding="lg" radius="2xl" class="flex flex-col items-start gap-3">
				<div>
					<p class="m-0 font-semibold">No DJs listed yet.</p>
					<p class="m-0 mt-1 text-fg-muted">Be the first on the map.</p>
				</div>
				<Button size="sm" href="/for-djs">Get listed</Button>
			</Card>
		{:else}
			<!-- A row to swipe on a phone, so the section stays about one screen tall. -->
			<ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(min(100%,18rem),1fr))] gap-4 p-0 max-sm:-mx-4 max-sm:flex max-sm:snap-x max-sm:snap-mandatory max-sm:scroll-px-4 max-sm:overflow-x-auto max-sm:px-4 max-sm:pb-2">
				{#each data.recent as project (project.uri)}
					<li class="max-sm:w-[82%] max-sm:shrink-0 max-sm:snap-start"><PerformerCard performer={projectSummary(project)} /></li>
				{/each}
			</ul>
		{/if}
	</div>
</Section>

{#if data.genres.length || data.cities.length}
	<Section tone="sunken" labelledby="browse-title">
		<div class="flex flex-col gap-8 sm:gap-12">
			<SectionHeader id="browse-title" tone="mid" title="Find DJs by sound or city" />
			<div class="grid gap-8 md:grid-cols-2 md:gap-12">
				{#if data.genres.length}
					<div class="flex flex-col gap-3">
						<h3 class="m-0 text-body font-semibold">By sound</h3>
						<ul class="m-0 flex list-none flex-wrap gap-2 p-0">
							{#each data.genres as genre (genre.value)}
								<li><Tag href="/projects?genres={encodeURIComponent(genre.value)}">{genre.value} <span class="font-mono text-caption text-fg-subtle">{genre.count}</span></Tag></li>
							{/each}
						</ul>
					</div>
				{/if}
				{#if data.cities.length}
					<div class="flex flex-col gap-3">
						<h3 class="m-0 text-body font-semibold">By city</h3>
						<ul class="m-0 flex list-none flex-wrap gap-2 p-0">
							{#each data.cities as city (city.city + city.country)}
								<li><Tag href="/projects?q={encodeURIComponent(city.city)}&country={city.country}">{city.city}, {city.country} <span class="font-mono text-caption text-fg-subtle">{city.count}</span></Tag></li>
							{/each}
						</ul>
					</div>
				{/if}
			</div>
		</div>
	</Section>
{/if}

<Section labelledby="promoters-title">
	<div class="flex flex-col gap-8 sm:gap-12">
		<SectionHeader id="promoters-title" tone="aqua" title="Describe the night. Get a shortlist with reasons." lede="The matcher scores every DJ on sound, distance, budget and room size, and tells you why each one made the list." />
		<ul class="m-0 grid list-none grid-cols-[repeat(auto-fit,minmax(min(100%,15rem),1fr))] gap-4 p-0">
			{#each perks as perk (perk.href)}
				<li>
					<Card href={perk.href} tone="outline" padding="lg" radius="2xl" class="flex h-full flex-col gap-2 max-sm:p-4">
						<span class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
							<span class="text-emphasis font-semibold">{perk.title}</span>
							{#if perk.signIn && !data.viewer}<span class="inline-flex items-center gap-1 text-label text-fg-subtle"><LockKeyhole aria-hidden="true" class="size-3.5" />Sign in</span>{/if}
						</span>
						<span class="text-fg-muted">{perk.body}</span>
					</Card>
				</li>
			{/each}
		</ul>
	</div>
</Section>

<Section tone="sunken" labelledby="tools-title">
	<div class="flex flex-col gap-8 sm:gap-12">
		<SectionHeader id="tools-title" tone="high" title="Tools for DJs, from the crate to the booth." />
		<ul class="m-0 flex list-none flex-wrap gap-4 p-0">
			{#each data.tools as tool (tool.id)}
				<li class="flex min-w-0 flex-[1_1_26rem]">
					<Card href="/tools/{tool.id}" padding="none" radius="2xl" class="flex w-full flex-col overflow-hidden">
						<span class="flex flex-col gap-2 p-5 sm:p-6">
							<span class="flex flex-wrap items-baseline gap-x-2.5 text-heading font-semibold">{tool.name} <span class="font-mono text-caption font-normal text-fg-subtle">{tool.kind}</span></span>
							<span class="text-fg-muted">{tool.tagline}</span>
						</span>
						<span class="mt-auto flex flex-col border-t border-line bg-canvas px-5 py-2 sm:px-6">
							{#each tool.lines as line (line)}<span class="border-b border-well py-2 text-small text-fg-soft last:border-b-0">{line}</span>{/each}
						</span>
					</Card>
				</li>
			{/each}
		</ul>
	</div>
</Section>

<Section tone="grid" labelledby="closing-title">
	<SectionHeader id="closing-title" align="center" title="You play out? Get on the map." lede="List yourself, a b2b duo or a whole crew. Already listed by an admin? Claim it. A moderator reviews every change.">
		{#snippet actions()}<Button size="lg" href="/dashboard/projects/new">Get listed</Button><Button size="lg" variant="secondary" href="/projects?unclaimed=on">Claim yours</Button><Button size="lg" variant="ghost" href="/for-djs">How it works for DJs</Button>{/snippet}
	</SectionHeader>
</Section>
