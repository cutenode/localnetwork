<script lang="ts">
	import HeroHeadline from '#lib/components/HeroHeadline.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import { projectSummary } from '#lib/performers.ts';
	import { pluralize } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Section from '#lib/ui/Section.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const pillars = [
		['Found by sound', 'A search for house finds deep house and afro house too.'],
		['Matched by distance', 'Results sort by how far each DJ is from your venue.'],
		['Booked in private', 'Every inquiry is its own members-only thread.'],
		['Owned by the DJ', 'Profiles are records on the AT Protocol.']
	];

	const perks = [
		['/match', 'Match a show', 'Describe the night and get a shortlist, with the reasons each DJ made it.'],
		['/dashboard/bookings', 'Private inquiries', 'Each booking is its own thread, shared only with the project’s managers.'],
		['/dashboard/planner', 'Planner', 'Keep shortlists and build a lineup slot by slot, in a space only you can read.'],
		['/about#spaces', 'Booking details', 'Email, phone, rate notes, rider and hospitality, for signed-in members.']
	];
</script>

<section aria-labelledby="hero-title" class="border-b border-line bg-canvas bg-grid">
	<div class="container-page flex flex-col items-center gap-8 py-14 sm:gap-10 sm:py-24">
		{#snippet headline()}<HeroHeadline />{/snippet}
		<SectionHeader id="hero-title" level={1} size="display" align="center" title={headline} lede="Promoters find DJs by sound, distance and budget. DJs keep a profile they own, with their booking details kept private." />
		<form action="/projects" method="GET" role="search" aria-label="Find DJs" class="grid w-full max-w-3xl items-end gap-3 rounded-2xl border border-line bg-surface p-4 sm:grid-cols-[1fr_1fr_auto] sm:p-5">
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
		<p class="m-0 text-small text-fg-muted">Play out yourself? <a class="link" href="/dashboard/projects/new">List your project</a></p>
	</div>
</section>

<div class="border-b border-line bg-canvas">
	<div class="container-page">
		<ul class="m-0 grid list-none grid-cols-2 gap-px bg-line p-0 lg:grid-cols-4">
			{#each pillars as [title, body] (title)}<li class="bg-canvas px-4 py-5 sm:px-6 sm:py-7"><p class="m-0 text-small font-semibold sm:text-body">{title}</p><p class="m-0 mt-1 text-label text-fg-muted sm:text-small">{body}</p></li>{/each}
		</ul>
	</div>
</div>

<Section labelledby="recent-title">
	<div class="flex flex-col gap-8 sm:gap-12">
		<div class="flex flex-wrap items-end justify-between gap-x-8 gap-y-4">
			<div class="flex flex-col gap-3">
				<SectionHeader id="recent-title" eyebrow="DJ profiles" tone="low" title="New on the directory" />
				{#if data.stats?.active}
					<p class="m-0 text-lede text-fg-muted">
						{pluralize(data.stats.active, 'project')} in {pluralize(data.stats.cities, 'city', 'cities')}{#if data.stats.unclaimed}, <a class="link" href="/projects?unclaimed=on">{data.stats.unclaimed} up for claiming</a>{/if}.
					</p>
				{/if}
			</div>
			{#if data.recent.length}<Button variant="secondary" href="/projects?sort=recent">See all projects</Button>{/if}
		</div>
		{#if !data.stats}
			<Card tone="outline" padding="lg" radius="2xl">
				<p class="m-0 font-semibold">The directory isn't answering right now.</p>
				<p class="m-0 mt-1 text-fg-muted">Try again in a minute.</p>
			</Card>
		{:else if data.recent.length === 0}
			<Card tone="outline" padding="lg" radius="2xl" class="flex flex-col items-start gap-3">
				<div>
					<p class="m-0 font-semibold">No projects listed yet.</p>
					<p class="m-0 mt-1 text-fg-muted">Be the first DJ on the map.</p>
				</div>
				<Button size="sm" href="/dashboard/projects/new">List your project</Button>
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
			<SectionHeader id="browse-title" eyebrow="Browse" tone="mid" title="Find DJs by sound or city" />
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
		<SectionHeader id="promoters-title" eyebrow="For promoters" tone="aqua" title="Describe the night. Get a shortlist with reasons." lede="The matcher scores every DJ on sound, distance, budget and room size, and tells you why each one made the list." />
		<ul class="m-0 grid list-none grid-cols-[repeat(auto-fit,minmax(min(100%,15rem),1fr))] gap-4 p-0">
			{#each perks as [href, title, body] (href)}
				<li><Card {href} tone="outline" padding="lg" radius="2xl" class="flex h-full flex-col gap-2 max-sm:p-4"><span class="text-emphasis font-semibold">{title}</span><span class="text-fg-muted">{body}</span></Card></li>
			{/each}
		</ul>
	</div>
</Section>

<Section tone="sunken" labelledby="tools-title">
	<div class="flex flex-col gap-8 sm:gap-12">
		<SectionHeader id="tools-title" eyebrow="Tools for DJs" tone="high" title="From the crate to the booth." />
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
	<SectionHeader id="closing-title" align="center" title="You play out? Put your project on the map." lede="List a solo act, a b2b duo or a whole crew. Already listed by an admin? Claim it. Every change is reviewed by a moderator.">
		{#snippet actions()}<Button size="lg" href="/dashboard/projects/new">List your project</Button><Button size="lg" variant="secondary" href="/projects?unclaimed=on">Claim a project</Button><Button size="lg" variant="ghost" href="/for-djs">How it works for DJs</Button>{/snippet}
	</SectionHeader>
</Section>
