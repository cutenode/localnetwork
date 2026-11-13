<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import type { HomeEvent } from '#lib/home-events.ts';
	import { EVENT_STATUS, isCancelled, venueClock, venueDay } from '#lib/shows.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import { webHref } from '#lib/web-url.ts';

	/**
	 * An event on the home page: its date, name, time and venue, and its lineup's faces, each linking its act. Events have
	 * no page yet, so the name links the first act's. `lineup` false leaves the lineup off where the line above already
	 * names it. Going, Interested and your network's count go under the lineup once there are RSVPs.
	 */
	let { event, wide = false, lineup = true }: { event: HomeEvent; wide?: boolean; lineup?: boolean } = $props();

	const record = $derived(event.shows[0].record);

	const cancelled = $derived(isCancelled(record));
</script>

<article class={['card flex h-full gap-4 p-4 sm:p-5', wide ? 'items-start' : 'flex-col']}>
	<DateBlock date={venueDay(record.startsAt)} tentative={cancelled} />
	<div class="flex min-w-0 flex-1 flex-col gap-1">
		<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
			<a class={['entity-link text-emphasis font-semibold break-words', cancelled && 'text-fg-muted line-through']} href="/projects/{event.shows[0].project.slug}">{record.name}</a>
			{#if cancelled}<span class="pill bg-raised text-critical">Cancelled</span>{:else if record.status === EVENT_STATUS.rescheduled}<span class="pill bg-raised text-fg-soft">New date</span>{/if}
		</p>
		<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
			<span class="font-mono">{venueClock(record.startsAt)}</span>
			<span aria-hidden="true">·</span><span>{[record.venue, record.location.city].filter(Boolean).join(', ')}</span>
		</p>
		{#if lineup}
			<ul aria-label="Lineup" class="m-0 mt-2 flex list-none flex-wrap gap-x-3 gap-y-1.5 p-0">
				{#each event.shows as show (show.uri)}
					<li><a class="entity-link inline-flex items-center gap-2 text-small text-fg-soft" href="/projects/{show.project.slug}"><Avatar src={show.project.avatarUrl} name={show.project.profile.name} size="sm" />{show.project.profile.name}</a></li>
				{/each}
			</ul>
		{/if}
		{#if record.tickets && !cancelled}<a class="btn-ghost btn-sm -ml-2.5 gap-1.5 self-start px-2.5" href={webHref(record.tickets)} rel="noopener noreferrer nofollow" target="_blank">Tickets<ExternalLink class="size-3.5" /></a>{/if}
	</div>
</article>
