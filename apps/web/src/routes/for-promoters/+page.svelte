<script lang="ts">
	import { CircleCheck } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import Screen from '#lib/components/landing/Screen.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import ScoreBar from '#lib/components/ScoreBar.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { feeSummary, placeLabel, pluralize } from '#lib/format.ts';
	import { projectSummary } from '#lib/performers.ts';
	import Button from '#lib/ui/Button.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import Section from '#lib/ui/Section.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	/** What Find DJs searches on, each the part of a listing it reads. */
	const searched = [
		['Sound', 'Genres match by family: a search for house finds deep house and afro house too.'],
		['Distance', 'From your venue, 50 km by default. DJs who travel further come up too.'],
		['Budget', 'Fee tiers from community to headliner. Amounts are optional.'],
		['Slot and setup', 'Opener to closing, the room sizes they suit, and vinyl, CDJs or live hardware.']
	];

	/** Made-up threads, since a real one is private to the people in it. */
	const threads = [
		{ title: 'Warehouse closing set', with: 'Halflight · Sat 14 Feb', status: 'negotiating' },
		{ title: 'Rooftop opener', with: 'Neon Tide · Fri 6 Mar', status: 'confirmed' },
		{ title: 'Basement residency', with: 'Okon · Thu 19 Mar', status: 'open' }
	];

	const lineup = [
		{ name: 'Neon Tide', slot: '22:00 opener', status: 'confirmed' },
		{ name: 'Halflight', slot: '00:00 peak time', status: 'contacted' },
		{ name: 'Okon', slot: '02:00 closing', status: 'considering' }
	];

	/** What a press kit holds for a promoter. */
	const kits = [
		['One per kind of booking', 'A club night and a festival can get different bios, mixes and photos.'],
		['Contact and rider', 'Booking email, rates and tech rider, shown once the DJ lets you in.'],
		['Kept current', 'The DJ publishes changes to the kit itself, so you see the version they published last.'],
		['Let in as a promoter', 'A DJ can open their kits to every promoter, or answer each request.']
	];

	const soon = [
		['Announce a night', 'The date, the venue and the lineup, with each DJ confirming their credit.'],
		["See who's going", 'Going and Interested, and how many of your network are in.'],
		['Reach your network', 'The people who follow you hear about each new night.']
	];

	const signIn = `/login?next=${encodeURIComponent('/dashboard/profile#section-promoter')}`;
</script>

<Section tone="grid" labelledby="hero-title">
	<div class="flex flex-col items-center gap-8 sm:gap-10">
		<SectionHeader id="hero-title" level={1} size="display" align="center" title="Find DJs who fit the night." lede="Search by sound, distance and budget, or describe your show and get a shortlist. Every booking is a private thread." />
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
		<p class="m-0 text-small text-fg-muted">Play out yourself? <a class="link" href="/for-djs">For DJs</a></p>
	</div>
</Section>

<Section labelledby="find-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<SectionHeader id="find-title" align="split" title="Search the way you book." lede="Find DJs reads what promoters book on. Every DJ in it was reviewed by staff before it went up.">
			{#snippet actions()}<Button variant="secondary" href="/projects">Open Find DJs</Button>{/snippet}
		</SectionHeader>
		<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
			<Screen path="/projects">
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
				{#snippet caption()}
					{#await data.directory}
						A listing from the directory, as it shows in Find DJs.
					{:then directory}
						A listing from the directory, as it shows in Find DJs.{directory?.djs ? ` ${pluralize(directory.djs, 'DJ')} in ${pluralize(directory.cities, 'city', 'cities')} so far.` : ''}
					{/await}
				{/snippet}
			</Screen>
			<dl class="m-0 grid gap-x-8 sm:grid-cols-2">
				{#each searched as [term, detail] (term)}
					<div class="flex flex-col gap-1 border-t border-line py-4">
						<dt class="font-semibold">{term}</dt>
						<dd class="m-0 text-fg-muted">{detail}</dd>
					</div>
				{/each}
			</dl>
		</div>
	</div>
</Section>

<Section tone="sunken" labelledby="match-title">
	<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
		<SectionHeader id="match-title" title="Describe the night. Get a shortlist with reasons." lede="Match a show scores every listed DJ on sound, distance, budget, room size, slot and setup, and says why each one made the list. Tick the ones you like to start a lineup.">
			{#snippet actions()}
				{#await data.shortlist}
					<Button href="/match">Match a show</Button>
				{:then shortlist}
					<Button href={shortlist?.href ?? '/match'}>Match a show</Button>
				{/await}
			{/snippet}
		</SectionHeader>
		<Screen path="/match">
			<ol class="m-0 flex min-h-[calc(3*4.5rem)] list-none flex-col divide-y divide-line p-0" aria-label="Shortlist">
				{#await data.shortlist}
					{#each { length: 3 }, row (row)}
						<li class="flex items-center gap-3 py-3" aria-hidden="true"><span class="size-10 shrink-0 animate-pulse rounded-full bg-well"></span><span class="h-4 w-1/2 animate-pulse rounded-md bg-raised"></span></li>
					{/each}
				{:then shortlist}
					{#each shortlist?.results ?? [] as result, index (result.candidate.uri)}
						{@const project = result.candidate}
						<li class="flex flex-wrap items-center gap-x-6 gap-y-2 py-3">
							<div class="flex min-w-0 flex-[1_1_16rem] items-center gap-3">
								<span class="w-4 shrink-0 font-mono text-caption text-fg-subtle">{index + 1}</span>
								<Avatar src={project.avatarUrl} name={project.profile.name} />
								<div class="flex min-w-0 flex-1 flex-col gap-0.5">
									<a class="font-semibold text-fg no-underline hover:underline" href="/projects/{project.slug}">{project.profile.name}</a>
									<p class="m-0 truncate text-label text-fg-muted">{[placeLabel(project.profile), feeSummary(project.profile)].filter(Boolean).join(' · ')}</p>
									<ul class="m-0 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-small text-fg-soft">
										{#each result.reasons.slice(0, 3) as reason (reason)}<li class="inline-flex items-center gap-1.5"><CircleCheck class="size-3.5 text-good" />{reason}</li>{/each}
									</ul>
								</div>
							</div>
							<ScoreBar score={result.score} />
						</li>
					{:else}
						<li class="py-3 text-small text-fg-muted">{shortlist ? 'The first matches will show here.' : "The directory isn't answering. Try again in a minute."}</li>
					{/each}
				{/await}
			</ol>
			{#snippet caption()}
				{#await data.shortlist}
					The matcher's answer, from the directory as it is now.
				{:then shortlist}
					{shortlist?.genre ? `The matcher's answer right now for ${shortlist.genre}${shortlist.place ? ` near ${shortlist.place}` : ''}.` : "The matcher's answer, from the directory as it is now."}
				{/await}
			{/snippet}
		</Screen>
	</div>
</Section>

<Section labelledby="inquiries-title">
	<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:gap-14">
		<Screen path="/dashboard/bookings" example>
			<ul class="m-0 flex list-none flex-col divide-y divide-line p-0">
				{#each threads as thread (thread.title)}
					<li class="flex items-center gap-3.5 py-3">
						<Monogram name={thread.with} />
						<div class="flex min-w-0 flex-1 flex-col gap-0.5">
							<p class="m-0 truncate font-semibold">{thread.title}</p>
							<p class="m-0 text-small text-fg-muted">{thread.with}</p>
						</div>
						<StatusPill status={thread.status} />
					</li>
				{/each}
			</ul>
			{#snippet caption()}Your booking threads, grouped by what needs you next.{/snippet}
		</Screen>
		<SectionHeader id="inquiries-title" title="One private thread per booking." lede="Send the date, venue, capacity, budget and slot up front. The thread is shared only with the people who answer for the DJ, and you both see where it stands, from open to confirmed." />
	</div>
</Section>

<Section tone="sunken" labelledby="planner-title">
	<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
		<SectionHeader id="planner-title" title="Shortlists and lineups only you can read." lede="Save DJs as you find them and build each night slot by slot. The planner lives in a space on your own PDS, and Local Network keeps no copy. Inquire from a planned show and its details come with you." />
		<Screen path="/dashboard/planner" example>
			<div class="flex flex-col gap-4">
				<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
					<p class="m-0 text-emphasis font-semibold">Warehouse, Sat 14 Feb</p>
					<p class="m-0 text-caption text-fg-subtle">Private to you</p>
				</div>
				<ol class="m-0 flex list-none flex-col divide-y divide-line p-0">
					{#each lineup as slot (slot.name)}
						<li class="flex items-center gap-3 py-3">
							<Monogram name={slot.name} size="sm" />
							<div class="flex min-w-0 flex-1 flex-col">
								<span class="truncate text-fg">{slot.name}</span>
								<span class="font-mono text-caption text-fg-subtle">{slot.slot}</span>
							</div>
							<StatusPill status={slot.status} />
						</li>
					{/each}
				</ol>
			</div>
			{#snippet caption()}A show in the planner, with its lineup as it comes together.{/snippet}
		</Screen>
	</div>
</Section>

<Section labelledby="kits-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<SectionHeader id="kits-title" align="split" title="Press kits with what you need to book." lede="Bio, mixes, photos, press, contact and rider, in one place the DJ keeps up to date.">
			{#snippet actions()}
				{#await data.directory then directory}
					{#if directory?.example}<Button variant="secondary" href="/projects/{directory.example.slug}/press-kits">See {directory.example.profile.name}'s press kits</Button>{/if}
				{/await}
			{/snippet}
		</SectionHeader>
		<dl class="m-0 grid gap-x-8 sm:grid-cols-2 lg:grid-cols-4">
			{#each kits as [term, detail] (term)}
				<div class="flex flex-col gap-1 border-t border-line py-4">
					<dt class="font-semibold">{term}</dt>
					<dd class="m-0 text-small text-fg-muted">{detail}</dd>
				</div>
			{/each}
		</dl>
		<div class="flex flex-col gap-4">
			<h3 class="m-0 text-emphasis font-semibold">Coming soon: events and RSVPs</h3>
			<dl class="m-0 grid gap-x-8 sm:grid-cols-2 lg:grid-cols-4">
				{#each soon as [term, detail] (term)}
					<div class="flex flex-col gap-1 border-t border-line py-4">
						<dt class="font-semibold">{term}</dt>
						<dd class="m-0 text-small text-fg-muted">{detail}</dd>
					</div>
				{/each}
			</dl>
		</div>
	</div>
</Section>

{#snippet turnOn()}
	<SectionHeader id="closing-title" align="center" title="Put on shows? Turn on promoting." lede="Sign in with your Atmosphere account and tick “I put on shows” on your profile.">
		{#snippet actions()}
			{#if data.viewer}<Button size="lg" href="/dashboard/profile#section-promoter">Turn on promoting</Button>{:else}<Button size="lg" href={signIn}>Sign in</Button>{/if}
			<Button size="lg" variant="secondary" href="/projects">Find DJs</Button>
		{/snippet}
	</SectionHeader>
{/snippet}

<Section tone="grid" labelledby="closing-title">
	{#await data.promoting}
		{@render turnOn()}
	{:then promoting}
		{#if promoting}
			<SectionHeader id="closing-title" align="center" title="You're set up to book." lede="Your planner and booking threads are on your dashboard.">
				{#snippet actions()}<Button size="lg" href="/match">Match a show</Button><Button size="lg" variant="secondary" href="/dashboard/planner">Your planner</Button>{/snippet}
			</SectionHeader>
		{:else}
			{@render turnOn()}
		{/if}
	{/await}
</Section>
