<script lang="ts">
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import ShowList from '#lib/components/ShowList.svelte';
	import Screen from '#lib/components/landing/Screen.svelte';
	import Capture from '#lib/components/tools/Capture.svelte';
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

	/** Why the profile is the DJ's own. */
	const owned = [
		['On your own account', 'Your profile, your sets and the shows you announce are records on your Atmosphere account. Any AT Protocol app can read them.'],
		['Sign in with what you have', 'A Bluesky account works, and so does an account at any other host.']
	];

	/** What a press kit does for a DJ. */
	const kits = [
		['One for each kind of booking', 'Up to 20: a club night and a festival can get different bios, mixes and photos.'],
		['You choose who sees them', 'Let in anyone signed in, promoters, people you follow or have played for, or only the people you approve.'],
		['Receipts', 'See who opened a press kit and when.'],
		['Shared with your crew', 'Everyone who manages the listing edits the same kits, and sees what the others changed.']
	];

	const soon = [
		["Who's coming", 'Going and Interested on each of your nights, and how many of their network are in.'],
		['Lineup credits', 'A promoter adds you to a lineup and you confirm it from your inbox.'],
		['Your followers hear first', 'People who follow you see the nights you play.']
	];

	/** How to get onto Find DJs, in order. */
	const steps = [
		['Sign in', 'With your Atmosphere account.'],
		['Turn on the DJ section', 'Tick “I play out as a DJ” on your profile and fill in your sound, city, slots and fees.'],
		['Ask to be listed', 'Staff look at your profile and list you in Find DJs. Playing as a duo, crew or alias? List it as its own act.']
	];
</script>

{#snippet calls()}
	<Button size="lg" href="/dashboard/profile#section-performer">Get listed</Button>
	<Button size="lg" variant="secondary" href="/projects?unclaimed=1">Find and claim yours</Button>
{/snippet}

<Section tone="grid" labelledby="hero-title">
	<SectionHeader
		id="hero-title"
		level={1}
		size="display"
		align="center"
		title="Get booked by promoters near you."
		lede="Keep a profile you own, get found in Find DJs, and take every booking in its own private thread. You decide what gets published."
		actions={calls}
	/>
</Section>

<Section labelledby="features-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<SectionHeader id="features-title" title="Built around how promoters book." />
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
						A listing from the directory, as promoters see it in search.
					{:then directory}
						{#if directory?.djs}A listing from the directory, as promoters see it. {pluralize(directory.djs, 'DJ')} in {pluralize(directory.cities, 'city', 'cities')} so far{#if directory.unclaimed}, <a class="link" href="/projects?unclaimed=1">{directory.unclaimed} waiting to be claimed</a>{/if}.{:else}A listing from the directory, as promoters see it in search.{/if}
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

<Section tone="sunken" labelledby="owned-title">
	<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
		<SectionHeader id="owned-title" title="A profile that's yours." lede="Local Network is built on the AT Protocol, so what you publish lives on your own account." />
		<dl class="m-0 grid gap-x-8 sm:grid-cols-2">
			{#each owned as [term, detail] (term)}
				<div class="flex flex-col gap-1 border-t border-line py-4">
					<dt class="font-semibold">{term}</dt>
					<dd class="m-0 text-small text-fg-muted">{detail}</dd>
				</div>
			{/each}
		</dl>
	</div>
</Section>

<Section labelledby="kits-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<SectionHeader id="kits-title" align="split" title="Press kits for every kind of booking." lede="Bio, mixes, photos, press quotes, contact and rider. The public part shows to everyone, and the rest to the people you let in.">
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
	</div>
</Section>

<Section tone="sunken" labelledby="locallibrary-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<SectionHeader id="locallibrary-title" align="split" title="locallibrary, from the crate to the booth." lede="A terminal app for rekordbox DJs. It keeps your USB drives in sync and checked for the players you'll meet, publishes your stats and sets to your profile, and turns a recorded set into a tracklist for your mix.">
			{#snippet actions()}
				<Button variant="secondary" href="/tools/locallibrary">About locallibrary</Button>
				<Button variant="ghost" href="/mixes/preview">Preview a mix export</Button>
			{/snippet}
		</SectionHeader>
		<div class="grid items-start gap-8 lg:grid-cols-2">
			{#each data.captures as { line, capture } (line)}
				<figure class="m-0 flex min-w-0 flex-col gap-3">
					<Capture {capture} sizes="(min-width: 1024px) 50vw, 100vw" class="card h-auto w-full" />
					<figcaption class="text-small text-fg-muted">{line}.</figcaption>
				</figure>
			{/each}
		</div>
	</div>
</Section>

<Section labelledby="sets-title">
	<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
		<SectionHeader id="sets-title" title="Sets, mixes and the charts." lede="Every set you publish gets its own page, with the tracklist and a player when there's a recording. Mixes link their sets. The tracks you play count toward the charts, by city and by month, and fans can follow your act.">
			{#snippet actions()}<Button variant="secondary" href="/charts">See the charts</Button>{/snippet}
		</SectionHeader>
		<Screen path="/charts">
			<!-- Five rows from the start, so the list arriving doesn't move what's under it. -->
			<ol aria-label="Most played this month" class="m-0 flex min-h-[calc(5*3.25rem)] list-none flex-col p-0">
				{#await data.tracks}
					{#each { length: 5 }, row (row)}<li class="flex h-13 items-center border-b border-line last:border-b-0"><span class="h-4 w-2/3 animate-pulse rounded-md bg-raised"></span></li>{/each}
				{:then tracks}
					{#each tracks ?? [] as track, index (track.id)}
						<li class="flex h-13 items-center gap-4 border-b border-line last:border-b-0">
							<span class="w-4 font-mono text-caption text-fg-subtle tabular-nums" aria-hidden="true">{index + 1}</span>
							<span class="flex min-w-0 flex-1 flex-col">
								<span class="truncate text-small font-semibold">{track.title}</span>
								{#if track.artist}<span class="truncate text-label text-fg-muted">{track.artist}</span>{/if}
							</span>
							<span class="font-mono text-caption whitespace-nowrap text-fg-muted tabular-nums">{pluralize(track.supports, 'DJ')}</span>
						</li>
					{:else}
						<li class="py-3 text-small text-fg-muted">{tracks ? 'Nothing played this month yet.' : "The charts aren't answering. Try again in a minute."}</li>
					{/each}
				{/await}
			</ol>
			{#snippet caption()}The month's most played tracks, from the sets DJs here published.{/snippet}
		</Screen>
	</div>
</Section>

<Section tone="sunken" labelledby="shows-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
			<SectionHeader id="shows-title" title="Announce your shows." lede="Add a show from your dashboard, or straight from a confirmed booking. It's a calendar event on your own account, so calendar apps like Smoke Signal show it too, and the people who follow your act find it on Shows.">
				{#snippet actions()}<Button variant="secondary" href="/shows">See what's on</Button>{/snippet}
			</SectionHeader>
			<Screen path="/shows">
				{#await data.shows}
					<ShowList shows={undefined} rows={3} empty={{ title: '' }} />
				{:then shows}
					<ShowList shows={shows?.slice(0, 3) ?? null} retryHref="/for-djs" empty={{ title: 'No shows announced yet', body: 'DJs announce theirs from their dashboard.' }} />
				{/await}
				{#snippet caption()}The next shows DJs here announced.{/snippet}
			</Screen>
		</div>
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

<Section labelledby="steps-title">
	<div class="grid items-start gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-14">
		<SectionHeader id="steps-title" title="How listing works." lede="Profiles go up as soon as you save them. Find DJs shows the ones staff have looked at." />
		<ol class="m-0 grid list-none gap-x-8 p-0 sm:grid-cols-3">
			{#each steps as [term, detail], index (term)}
				<li class="flex flex-col gap-1 border-t border-line py-4">
					<span class="font-mono text-caption text-fg-subtle">{index + 1}</span>
					<span class="font-semibold">{term}</span>
					<span class="text-small text-fg-muted">{detail}</span>
				</li>
			{/each}
		</ol>
	</div>
</Section>

<Section tone="grid" labelledby="closing-title">
	<SectionHeader id="closing-title" align="center" title="Already on Local Network?" lede="An admin may have listed you already. Claim it, or get listed yourself. A moderator reviews both." actions={calls} />
</Section>
