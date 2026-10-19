<script lang="ts">
	import { page } from '$app/state';
	import NearInput from '#lib/components/NearInput.svelte';
	import ShowList from '#lib/components/ShowList.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const place = $derived(data.place);

	const viewer = $derived(data.viewer);

	const tabs = [
		{ label: 'Near you', href: '/shows', following: false },
		{ label: 'Following', href: '/shows?view=following', following: true }
	];

	const chip = (active: boolean) => ['chip min-h-9 px-3 transition-colors hover:border-fg-subtle', active && 'border-fg bg-fg font-semibold text-fg-inverse hover:border-fg'];

	const heading = $derived(
		data.following
			? `From DJs you follow${place ? `, nearest ${place.place} first` : ''}`
			: place
				? `Within ${place.radiusKm} km of ${place.place}`
				: 'Everywhere'
	);

	const empty = $derived(
		data.following
			? viewer
				? { title: 'Nothing coming up from DJs you follow', body: 'Follow DJs on their pages and their shows land here.' }
				: { title: 'See shows from DJs you follow', body: 'Sign in, then follow DJs on their pages.' }
			: place
				? { title: `Nothing on near ${place.place} yet`, body: 'Try a wider radius, or look everywhere.' }
				: { title: 'No upcoming shows yet', body: 'When DJs announce shows, they show up here.' }
	);

	const here = $derived(page.url.pathname + page.url.search);

	const later = (cursor: string) => {
		const url = new URL(page.url.href);
		url.searchParams.set('cursor', cursor);

		return url.pathname + url.search;
	};
</script>

<svelte:head>
	<title>Shows · Local Network</title>
	<meta name="description" content="Upcoming shows by DJs on Local Network, near you or from the DJs you follow." />
</svelte:head>

{#snippet action()}
	{#if data.following && !viewer}
		<a class="btn-primary btn-sm" href="/login?next=/shows?view=following">Sign in</a>
	{:else if data.following}
		<a class="btn-secondary btn-sm" href="/projects">Find DJs</a>
	{:else if place}
		<a class="btn-secondary btn-sm" href="/shows?place=">Look everywhere</a>
	{/if}
{/snippet}

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Shows" title="Upcoming shows" align="split" lede="Where DJs on Local Network play next, near you or from the DJs you follow." />

	<div class="grid items-start gap-6 lg:grid-cols-[18rem_minmax(0,1fr)] lg:gap-10">
		<form method="GET" class="card flex flex-col gap-5 p-5 lg:sticky lg:top-20" data-sveltekit-reset="false">
			{#if data.following}<input type="hidden" name="view" value="following" />{/if}
			<div>
				<label class="label" for="near">City</label>
				<NearInput place={place?.place ?? ''} lat={place ? String(place.lat) : ''} lng={place ? String(place.lng) : ''} geocoder={data.geocoder} />
			</div>
			{#if !data.following}
				<div>
					<label class="label" for="radius">Within</label>
					<select id="radius" name="radius" class="input">
						{#each data.radii as km (km)}<option value={km} selected={(place?.radiusKm ?? 50) === km}>{km} km</option>{/each}
					</select>
				</div>
			{/if}
			<button class="btn-primary">{data.following ? 'Sort by distance' : 'Find shows'}</button>
			{#if place}<a class="self-start text-small text-fg-muted no-underline hover:text-fg" href="/shows?place={data.following ? '&view=following' : ''}">Forget this city</a>{/if}
		</form>

		<section aria-labelledby="results-title" class="flex min-w-0 flex-col gap-5">
			<nav aria-label="Which shows" class="flex flex-wrap gap-2">
				{#each tabs as tab (tab.href)}<a href={tab.href} aria-current={data.following === tab.following ? 'page' : undefined} class={chip(data.following === tab.following)}>{tab.label}</a>{/each}
			</nav>
			<h2 id="results-title" class="m-0 text-body font-semibold">{heading}</h2>
			{#await data.shows}
				<ShowList shows={undefined} rows={3} {empty} />
			{:then result}
				<ShowList shows={result?.items ?? null} {empty} retryHref={here} children={action} />
				{#if result?.cursor}<a class="btn-secondary self-start" href={later(result.cursor)}>Later shows</a>{/if}
			{/await}
		</section>
	</div>
</div>
