<script lang="ts">
	import Avatar from '#lib/components/Avatar.svelte';
	import FollowButton from '#lib/components/FollowButton.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import { pluralize } from '#lib/format.ts';
	import { eventsOf, type HomeArea } from '#lib/home-events.ts';
	import { venueDay } from '#lib/shows.ts';
	import type { ShowView, SupportedTrack } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import Section from '#lib/ui/Section.svelte';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import type { PageData } from '../../../routes/$types';
	import EventCard from './EventCard.svelte';

	/**
	 * The home page for going out. Signed out it leads with what's on near the place picked last, then the DJs there,
	 * their mixes and the charts, and what an account gets you. Signed in it leads with your network's shows, with
	 * what's near your home city beside them.
	 */
	let { data }: { data: Extract<PageData, { social: true }> } = $props();

	const viewer = $derived(data.viewer);

	/** How many events the hero shows beside the headline. */
	const WALL = 4;

	const MIXES = 4;

	const login = (next: string) => `/login?next=${encodeURIComponent(next)}`;

	const night = (show: ShowView) => new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC', weekday: 'short', day: 'numeric', month: 'short' }).format(new Date(`${venueDay(show.record.startsAt)}T00:00:00Z`));

	/** /shows near the same place, further out. */
	const further = ({ place, lat, lng }: HomeArea['place']) => `/shows?${new URLSearchParams({ place, lat: String(lat), lng: String(lng), radius: '250' })}`;

	/** The first mix of each of the area's DJs, linking its tracklist page when it has one and its act's page if not. */
	const mixesOf = (djs: NonNullable<HomeArea['djs']>) =>
		djs
			.flatMap(({ project }) => {
				const mix = project.profile.mixes?.[0];

				return mix ? [{ project, title: mix.title, href: `/projects/${project.slug}${mix.tracklist ? '/mixes/1' : ''}` }] : [];
			})
			.slice(0, MIXES);
</script>

{#snippet offline()}
	<div class="card p-5"><p class="m-0 font-semibold">Shows are offline</p><p class="m-0 mt-1 text-small text-fg-muted">The listings aren't answering. Try again in a minute.</p></div>
{/snippet}

{#snippet note(title: string, body: string, href: string, action: string)}
	<div class="card flex h-full flex-col items-start justify-center gap-3 p-5 sm:p-6">
		<div><p class="m-0 font-semibold">{title}</p><p class="m-0 mt-1 text-small text-fg-muted">{body}</p></div>
		<a class="btn-secondary btn-sm" {href}>{action}</a>
	</div>
{/snippet}

{#snippet ghosts(count: number, wide = false)}
	<ul aria-busy="true" aria-label="Loading shows" class={['m-0 grid list-none gap-3 p-0', !wide && 'sm:grid-cols-2']}>
		{#each { length: count }, index (index)}
			<li class="card flex min-h-40 gap-4 p-4 sm:p-5"><span class="h-18 w-12 shrink-0 animate-pulse rounded-lg bg-well sm:h-21 sm:w-16"></span><span class="flex flex-1 flex-col gap-2 pt-1"><span class="h-5 w-2/3 animate-pulse rounded-md bg-raised"></span><span class="h-4 w-1/2 animate-pulse rounded-md bg-raised"></span></span></li>
		{/each}
	</ul>
{/snippet}

{#snippet charts(tracks: SupportedTrack[] | null)}
	{#if !tracks?.length}
		<p class="m-0 py-3 text-small text-fg-muted">{tracks ? 'Nothing played this month yet.' : "The charts aren't answering. Try again in a minute."}</p>
	{:else}
		<ol aria-labelledby="played-title" class="m-0 flex list-none flex-col p-0">
			{#each tracks as track, index (track.id)}
				<li class="flex h-13 items-center gap-4 border-b border-line last:border-b-0">
					<span class="w-4 font-mono text-caption text-fg-subtle tabular-nums" aria-hidden="true">{index + 1}</span>
					<span class="flex min-w-0 flex-1 flex-col"><span class="truncate text-small font-semibold">{track.title}</span>{#if track.artist} <span class="truncate text-label text-fg-muted">{track.artist}</span>{/if}</span>
					<span class="font-mono text-caption whitespace-nowrap text-fg-muted tabular-nums">{pluralize(track.supports, 'DJ')}</span>
				</li>
			{/each}
		</ol>
	{/if}
{/snippet}

{#snippet played()}
	<div class="flex flex-col gap-3">
		<div class="flex flex-wrap items-end justify-between gap-2"><h2 id="played-title" class="m-0 text-heading font-semibold">Most played this month</h2><a class="link text-small" href="/charts">Charts</a></div>
		{#await data.tracks}
			<div role="status" class="h-65 animate-pulse rounded-xl bg-raised" aria-label="Loading the charts"></div>
		{:then tracks}
			{@render charts(tracks)}
		{/await}
	</div>
{/snippet}

{#if !viewer}
	<section aria-labelledby="hero-title" class="border-b border-line bg-canvas bg-grid">
		<div class="container-page grid grid-cols-[minmax(0,1fr)] items-center gap-10 py-12 sm:py-20 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:gap-16">
			<div class="flex min-w-0 flex-col gap-7">
				<h1 id="hero-title" class="m-0 text-display font-semibold text-balance">What's on near you, and who's going.</h1>
				<form method="GET" role="search" aria-label="What's on near" class="flex flex-col gap-2 rounded-2xl border border-line bg-surface p-2 sm:flex-row sm:items-start" data-sveltekit-reset="false">
					<div class="min-w-0 flex-1">
						<label class="sr-only" for="home-near">Near</label>
						<NearInput id="home-near" place={data.picked?.place ?? ''} lat={data.picked ? String(data.picked.lat) : ''} lng={data.picked ? String(data.picked.lng) : ''} geocoder={data.geocoder} />
					</div>
					<button class="btn-primary">Show what's on</button>
				</form>
				<p class="m-0 flex flex-wrap gap-x-5 gap-y-2 text-small text-fg-muted">
					<span>Play out? <a class="link" href="/for-djs">Local Network for DJs</a></span>
					<span>Book DJs? <a class="link" href="/for-promoters">Local Network for promoters</a></span>
				</p>
			</div>
			<div class="min-w-0">
				{#await data.area}
					{@render ghosts(WALL)}
				{:then area}
					{#if !area}
						{@render note('Pick your city', "Type a city above and see what's on there this week.", '/shows', 'Or see every show')}
					{:else if !area.shows}
						{@render offline()}
					{:else if !area.shows.length}
						{@render note(`Nothing announced near ${area.place.place} yet`, 'DJs announce their shows here as they book them.', further(area.place), 'Look further out')}
					{:else}
						<h2 class="sr-only">Coming up near {area.place.place}</h2>
						<!-- A row to swipe on a phone, so the hero stays about one screen tall. Relative, so the screen-reader-only text in it scrolls with it rather than widening the page. -->
						<ul class="m-0 grid list-none gap-3 p-0 sm:grid-cols-2 relative max-sm:-mx-4 max-sm:flex max-sm:snap-x max-sm:snap-mandatory max-sm:scroll-px-4 max-sm:overflow-x-auto max-sm:px-4 max-sm:pb-2">
							{#each eventsOf(area.shows).slice(0, WALL) as event (event.uri)}<li class="max-sm:w-[78%] max-sm:shrink-0 max-sm:snap-start"><EventCard {event} /></li>{/each}
						</ul>
						<a class="link mt-4 inline-block text-small" href="/shows">All shows near {area.place.place}</a>
					{/if}
				{/await}
			</div>
		</div>
	</section>

	{#await data.area then area}
		{#if area?.djs?.length}
			<Section labelledby="djs-title" space="md">
				<div class="flex flex-col gap-6">
					<div class="flex flex-wrap items-end justify-between gap-2"><h2 id="djs-title" class="m-0 text-title font-semibold">Who's playing near {area.place.place}</h2><a class="link text-small" href="/projects">All DJs</a></div>
					<!-- A row to swipe on a phone, like the events above, and relative for the same reason. -->
					<ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-3 p-0 relative max-sm:-mx-4 max-sm:flex max-sm:snap-x max-sm:snap-mandatory max-sm:scroll-px-4 max-sm:overflow-x-auto max-sm:px-4 max-sm:pb-2">
						{#each area.djs as { project, follow } (project.uri)}
							<li class="card flex flex-col items-center gap-3 p-4 text-center max-sm:w-44 max-sm:shrink-0 max-sm:snap-start">
								<Avatar src={project.avatarUrl} name={project.profile.name} size="xl" />
								<span class="flex min-w-0 flex-col gap-0.5">
									<a class="entity-link font-semibold break-words" href="/projects/{project.slug}">{project.profile.name}</a>
									{#if project.profile.genres[0]}<span class="text-label text-fg-muted">{project.profile.genres[0]}</span>{/if}
								</span>
								<FollowButton {follow} loginHref={login(`/projects/${project.slug}#follow`)} />
							</li>
						{/each}
					</ul>
				</div>
			</Section>
		{/if}
	{/await}

	<Section labelledby="played-title" space="md">
		<div class="grid grid-cols-[minmax(0,1fr)] gap-12 md:grid-cols-2">
			{#await data.area then area}
				{@const mixes = area?.djs ? mixesOf(area.djs) : []}
				{#if area && mixes.length}
					<div class="flex flex-col gap-3">
						<h2 class="m-0 text-heading font-semibold">Mixes from around {area.place.place}</h2>
						<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
							{#each mixes as mix (mix.project.uri)}
								<li><a class="card flex items-center gap-3 p-3 no-underline transition-colors hover:border-line-strong" href={mix.href}><Avatar src={mix.project.avatarUrl} name={mix.project.profile.name} /><span class="flex min-w-0 flex-col"><span class="truncate font-semibold text-fg">{mix.title}</span> <span class="truncate text-small text-fg-muted">{mix.project.profile.name}</span></span></a></li>
							{/each}
						</ul>
					</div>
				{/if}
			{/await}
			{@render played()}
		</div>
	</Section>

	<Section tone="sunken" labelledby="account-title" space="md">
		<div class="flex flex-col gap-6">
			<h2 id="account-title" class="m-0 text-title font-semibold">Your scene, in one place</h2>
			<ul class="m-0 grid list-none gap-x-8 gap-y-3 p-0 sm:grid-cols-3">
				<li><span class="font-semibold">Follow people and acts.</span> <span class="text-fg-muted">Their shows come to you.</span></li>
				<li><span class="font-semibold">See where your network is going.</span> <span class="text-fg-muted">Then go too.</span></li>
				<li><span class="font-semibold">Keep your plans.</span> <span class="text-fg-muted">Every night you said yes to, in one list.</span></li>
			</ul>
			<div><Button href={login('/')}>Sign in</Button></div>
		</div>
	</Section>
{:else}
	<div class="container-page grid grid-cols-[minmax(0,1fr)] gap-10 py-8 sm:py-12 lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-14">
		<section aria-labelledby="network-title" class="flex min-w-0 flex-col gap-5">
			<h1 id="network-title" class="m-0 text-title font-semibold">Your network</h1>
			{#await data.network}
				{@render ghosts(3, true)}
			{:then shows}
				{#if !shows}
					{@render offline()}
				{:else if !shows.length}
					{@render note('Follow DJs and acts to see their shows here', "Their next nights land on this page, soonest first.", '/projects', 'Find DJs')}
				{:else}
					<ol class="m-0 flex list-none flex-col gap-6 p-0">
						{#each eventsOf(shows) as event (event.uri)}
							{@const names = event.shows.map((show) => show.project.profile.name)}
							<li class="flex flex-col gap-2.5">
								<p class="m-0 flex items-center gap-2.5 text-small">
									<Avatar src={event.shows[0].project.avatarUrl} name={names[0]} size="sm" />
									<span><a class="entity-link font-semibold" href="/projects/{event.shows[0].project.slug}">{names[0]}</a><span class="text-fg-muted">{names.length > 1 ? ` and ${pluralize(names.length - 1, 'other')} play` : ' plays'} {night(event.shows[0])}</span></span>
								</p>
								<EventCard {event} wide lineup={names.length > 1} />
							</li>
						{/each}
					</ol>
				{/if}
			{/await}
		</section>
		<aside class="flex min-w-0 flex-col gap-10" aria-label="Near you">
			{#await data.area}
				<div role="status" class="h-48 animate-pulse rounded-xl bg-raised" aria-label="Loading what's near you"></div>
			{:then area}
				{#if !area}
					{@render note("See what's on near you", 'Set your home city and its shows land here.', '/dashboard/profile', 'Set your home city')}
				{:else}
					<section aria-labelledby="near-title" class="flex flex-col gap-4">
						<div class="flex items-end justify-between gap-2"><h2 id="near-title" class="m-0 text-heading font-semibold">Near {area.place.place}</h2><a class="link text-small" href="/shows">All shows</a></div>
						{#if !area.shows}
							{@render offline()}
						{:else if !area.shows.length}
							<p class="m-0 text-small text-fg-muted">Nothing else announced near {area.place.place} yet.</p>
						{:else}
							<ul class="m-0 flex list-none flex-col gap-3 p-0">
								{#each eventsOf(area.shows).slice(0, WALL) as event (event.uri)}
									<li class="flex items-start gap-3">
										<DateBlock date={venueDay(event.shows[0].record.startsAt)} compact />
										<span class="flex min-w-0 flex-col">
											<a class="entity-link font-semibold" href="/projects/{event.shows[0].project.slug}">{event.shows[0].record.name}</a>
											<span class="text-small text-fg-muted">{[event.shows[0].record.venue, ...event.shows.map((show) => show.project.profile.name)].filter(Boolean).join(' · ')}</span>
										</span>
									</li>
								{/each}
							</ul>
						{/if}
					</section>
					{#if area.djs?.length}
						<section aria-labelledby="djs-title" class="flex flex-col gap-4">
							<h2 id="djs-title" class="m-0 text-heading font-semibold">DJs near {area.place.place}</h2>
							<ul class="m-0 flex list-none flex-col gap-3 p-0">
								{#each area.djs as { project, follow } (project.uri)}
									<li class="flex flex-wrap items-center gap-3">
										<Avatar src={project.avatarUrl} name={project.profile.name} />
										<a class="entity-link min-w-0 flex-1 truncate font-semibold" href="/projects/{project.slug}">{project.profile.name}</a>
										<FollowButton {follow} page="/projects/{project.slug}" />
									</li>
								{/each}
							</ul>
						</section>
					{/if}
				{/if}
			{/await}
			{@render played()}
		</aside>
	</div>
{/if}
