<script lang="ts">
	import { Map as MapIcon, MapPin } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import { asShowDate, nightLabel } from '#lib/format.ts';
	import { EVENT_STATUS, showHours } from '#lib/shows.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const venue = $derived(data.venue);

	/** Its street, then city, region and country, as much as is known. */
	const address = $derived([venue.address.street, venue.address.locality, venue.address.region, venue.address.country].filter(Boolean).join(', '));

	const map = $derived(venue.geo && `https://www.openstreetmap.org/?mlat=${venue.geo.latitude}&mlon=${venue.geo.longitude}#map=17/${venue.geo.latitude}/${venue.geo.longitude}`);
</script>

<div class="border-b border-line">
	<div class="container-page flex flex-col gap-5 py-10 sm:flex-row sm:items-end sm:py-14">
		<Avatar name={venue.name} size="xl" />
		<div class="flex min-w-0 flex-1 flex-col gap-2">
			<h1 class="m-0 text-title font-semibold">{venue.name}</h1>
			<p class="m-0 flex flex-wrap items-center gap-x-4 gap-y-1 text-small text-fg-muted">
				<span class="inline-flex items-center gap-1.5"><MapPin aria-hidden="true" class="size-4" />{address}</span>
				{#if map}<a class="link inline-flex items-center gap-1.5" href={map} rel="noopener noreferrer" target="_blank"><MapIcon aria-hidden="true" class="size-4" />Map</a>{/if}
			</p>
		</div>
	</div>
</div>

<div class="container-page flex flex-wrap gap-x-10 gap-y-8 py-10">
	<section aria-labelledby="events" class="flex min-w-0 flex-[999_1_34rem] flex-col gap-3">
		<h2 id="events" class="m-0 eyebrow text-fg-subtle">Upcoming events</h2>
		{#if data.events.length}
			<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
				{#each data.events as event (event.uri)}
					{@const isCancelled = event.status === EVENT_STATUS.cancelled}
					<li class="card flex items-start gap-4 p-4">
						<div class="flex min-w-0 flex-1 flex-col gap-1">
							<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
								<a class={['entity-link text-emphasis font-semibold break-words', isCancelled && 'text-fg-muted line-through']} href={event.href}>{event.name}</a>
								{#if isCancelled}
									<span class="pill bg-raised text-critical">Cancelled</span>
								{:else}
									{#if event.status === EVENT_STATUS.rescheduled}<span class="pill bg-raised text-fg-soft">New date</span>{/if}
									<span class={['pill', event.confirmed ? 'bg-raised text-good' : 'bg-well text-fg-muted']}>{event.confirmed ? 'Confirmed' : 'Waiting for the venue'}</span>
								{/if}
							</p>
							<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
								<span>{nightLabel(asShowDate(event.startsAt))}</span><span class="font-mono">{showHours(event)}</span>
								<span aria-hidden="true">·</span><a class="entity-link text-fg-soft" href={event.party.href}>{event.party.name}</a>
							</p>
						</div>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="card m-0 p-5 text-small text-fg-muted">No upcoming events here yet.</p>
		{/if}
	</section>
	{#if data.dashboard.length}
		<aside class="flex min-w-0 flex-[1_1_18rem] flex-col gap-6">
			<section aria-labelledby="dashboard" class="card flex flex-col gap-3 p-5">
				<h2 id="dashboard" class="m-0 eyebrow text-fg-subtle">Dashboard</h2>
				<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
					{#each data.dashboard as link (link.href)}<li><a class="link" href={link.href}>{link.label}</a></li>{/each}
				</ul>
			</section>
		</aside>
	{/if}
</div>
