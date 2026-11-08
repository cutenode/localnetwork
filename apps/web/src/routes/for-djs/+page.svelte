<script lang="ts">
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import { pluralize } from '#lib/format.ts';
	import { projectSummary } from '#lib/performers.ts';
	import Button from '#lib/ui/Button.svelte';
	import Section from '#lib/ui/Section.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	/** What promoters search and match on, each the part of a listing it reads. */
	const searched = [
		['Sound', 'Your genres, matched by family: a search for house finds your deep house and afro house sets too.'],
		['Distance', 'Your city. Promoters search within 50 km of their venue by default, and your travel radius brings in gigs further out.'],
		['Fee', 'Your fee tier, matched against their budget. Amounts are optional.'],
		['Slot and room', 'The slots you play and the room sizes that suit you, from opener to closing in a 1,500 cap room.']
	];

	/** What happens once a promoter has found you. */
	const after = [
		['Inquiries with the details', 'Every inquiry arrives with the date, venue, capacity, budget and slot. Reply and confirm in a private thread.'],
		['Crews and agents', 'Up to ten people can answer for one listing, so duos, collectives and booking agents all can.'],
		['Private details stay private', 'Your booking email, rates and tech rider show only to people you let into a press kit, and you see when they look.'],
		['Reviewed changes', 'Moderators review every claim and edit, so nobody else can take over or deface your page.']
	];
</script>

{#snippet calls()}
	<Button size="lg" href="/dashboard/projects/new">Get listed</Button>
	<Button size="lg" variant="secondary" href="/projects?unclaimed=1">Find and claim yours</Button>
{/snippet}

<Section tone="grid" labelledby="hero-title">
	<SectionHeader
		id="hero-title"
		level={1}
		size="display"
		align="center"
		title="Get booked by promoters near you."
		lede="Promoters search by sound, distance, slot, room size and budget, and your profile covers all of them. You decide what gets published."
		actions={calls}
	/>
</Section>

<Section labelledby="features-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<SectionHeader id="features-title" tone="low" title="Built around how promoters book." />
		<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
			<figure class="m-0 flex flex-col gap-3">
				<!-- The height a card takes, so the listing arriving doesn't move the list beside it. -->
				<div class="min-h-37">
					{#await data.directory}
						<!-- Named by its text: an aria-label on a plain div is dropped. -->
						<div class="card flex h-37 flex-col gap-3 p-5" role="status" aria-busy="true">
							<span class="sr-only">Loading a listing</span>
							<div class="flex items-center gap-3"><div class="size-12 animate-pulse rounded-full bg-well"></div><div class="h-5 w-1/2 animate-pulse rounded-md bg-raised"></div></div>
							<div class="h-4 w-3/4 animate-pulse rounded-md bg-raised"></div>
						</div>
					{:then directory}
						{#if directory?.example}
							<PerformerCard performer={projectSummary(directory.example)} />
						{:else}
							<div class="card flex h-37 items-center p-5 text-small text-fg-muted">{directory ? 'The first listing will show here.' : "The directory isn't answering. Try again in a minute."}</div>
						{/if}
					{/await}
				</div>
				<figcaption class="text-small text-fg-muted">
					{#await data.directory}
						A listing from the directory, as promoters see it in search.
					{:then directory}
						{#if directory?.djs}A listing from the directory, as promoters see it. {pluralize(directory.djs, 'DJ')} in {pluralize(directory.cities, 'city', 'cities')} so far{#if directory.unclaimed}, <a class="link" href="/projects?unclaimed=1">{directory.unclaimed} waiting to be claimed</a>{/if}.{:else}A listing from the directory, as promoters see it in search.{/if}
					{/await}
				</figcaption>
			</figure>
			<dl class="m-0 grid gap-x-8 sm:grid-cols-2">
				{#each searched as [term, detail] (term)}
					<div class="flex flex-col gap-1 border-t border-line py-4">
						<dt class="font-semibold">{term}</dt>
						<dd class="m-0 text-fg-muted">{detail}</dd>
					</div>
				{/each}
			</dl>
		</div>
		<div class="flex flex-col gap-4">
			<h3 class="m-0 text-emphasis font-semibold">Once they've found you</h3>
			<dl class="m-0 grid gap-x-8 sm:grid-cols-2 lg:grid-cols-4">
				{#each after as [term, detail] (term)}
					<div class="flex flex-col gap-1 border-t border-line py-4">
						<dt class="font-semibold">{term}</dt>
						<dd class="m-0 text-small text-fg-muted">{detail}</dd>
					</div>
				{/each}
			</dl>
		</div>
	</div>
</Section>

<Section tone="sunken" labelledby="mixes-title">
	<SectionHeader
		id="mixes-title"
		align="split"
		tone="mid"
		title="Tracklists for your mixes."
		lede="Export a mix from locallibrary and attach it to the mix on your profile. Visitors see what you played, the tempo, the key moves and every transition."
	>
		{#snippet actions()}
			<Button variant="secondary" href="/mixes/preview">Preview an export</Button>
			<Button variant="ghost" href="/tools/locallibrary">About locallibrary</Button>
		{/snippet}
	</SectionHeader>
</Section>

<Section tone="grid" labelledby="closing-title">
	<SectionHeader id="closing-title" align="center" title="Already on Local Network?" lede="An admin may have listed you already. Claim it, or get listed yourself. A moderator reviews both." actions={calls} />
</Section>
