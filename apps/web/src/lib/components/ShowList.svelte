<script lang="ts">
	import { ExternalLink, Pencil } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import type { Listing } from '#lib/events.ts';
	import { formatDistance } from '#lib/geo.ts';
	import { EVENT_STATUS, isCancelled, showHours, venueDay } from '#lib/shows.ts';
	import type { ShowView } from '#lib/types.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import { webHref } from '#lib/web-url.ts';

	/**
	 * Upcoming shows and parties' events, soonest (or nearest) first, in every state a listing has: `shows` undefined
	 * while they load (as `rows` placeholders), null when the directory can't answer, empty with `empty`'s title and
	 * body and `children` as its action. Each state is at least a show's height, so one show, none, or the placeholder
	 * take the same room. `project` says whose shows these are, which leaves each show's project off. An event links its
	 * page, party and venue, and says whether the venue has confirmed it. `editable` gives the viewer's own shows an edit
	 * link, and `edited` marks the ones another app changed since.
	 */
	let {
		shows,
		project,
		editable,
		edited = [],
		rows = 1,
		empty,
		retryHref,
		children
	}: {
		shows: Listing[] | null | undefined;
		project?: string;
		editable?: (show: ShowView) => string | null;
		edited?: string[];
		rows?: number;
		empty: { title: string; body?: string };
		retryHref?: string;
		children?: Snippet;
	} = $props();

	/** What a row shows, the same way for a show and an event: its project or party, and its venue and city. */
	function rowOf(item: Listing) {
		if ('kind' in item) return { record: item, href: item.href, by: item.party, venue: item.venue, city: item.venue.locality, confirmed: item.confirmed, edit: null };

		const { record } = item;
		const by = project === item.project.uri ? null : { name: item.project.profile.name, href: `/projects/${item.project.slug}` };

		return { record, href: null, by, venue: record.venue ? { name: record.venue, href: null } : null, city: record.location.city, confirmed: null, edit: editable?.(item) ?? null };
	}
</script>

{#snippet note(title: string, body: string | undefined, action: Snippet | undefined)}
	<div class="card flex min-h-26 flex-wrap items-center justify-between gap-x-6 gap-y-3 p-4 sm:min-h-29 sm:px-5">
		<div class="flex flex-col gap-1">
			<p class="font-semibold">{title}</p>
			{#if body}<p class="text-small text-fg-muted">{body}</p>{/if}
		</div>
		{#if action}{@render action()}{/if}
	</div>
{/snippet}

{#snippet retry()}<a class="btn-secondary btn-sm" href={retryHref}>Try again</a>{/snippet}

{#if shows === undefined}
	<ol aria-busy="true" aria-label="Loading shows" class="flex flex-col gap-2.5">
		{#each { length: rows }, row (row)}
			<li class="card flex min-h-26 items-start gap-4 p-4 sm:min-h-29">
				<div class="h-18 w-12 shrink-0 animate-pulse rounded-lg bg-well sm:h-21 sm:w-16"></div>
				<div class="flex flex-1 flex-col gap-2 pt-1">
					<div class="h-5 w-2/3 animate-pulse rounded-md bg-raised"></div>
					<div class="h-4 w-1/2 animate-pulse rounded-md bg-raised"></div>
				</div>
			</li>
		{/each}
	</ol>
{:else if shows === null}
	{@render note('Shows are offline', "The listings aren't answering. Try again in a minute.", retryHref ? retry : undefined)}
{:else if shows.length === 0}
	{@render note(empty.title, empty.body, children)}
{:else}
	<ol class="flex flex-col gap-2.5">
		{#each shows as item (item.uri)}
			{@const { record, href, by, venue: place, city, confirmed, edit } = rowOf(item)}
			{@const cancelled = isCancelled(record)}
			<li class="card flex min-h-26 items-start gap-4 p-4 sm:min-h-29">
				<DateBlock date={venueDay(record.startsAt)} tentative={confirmed === false} />
				<div class="flex min-w-0 flex-1 flex-col gap-1">
					<p class="flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
						{#if href}<a class={['entity-link text-emphasis break-words', cancelled && 'text-fg-muted line-through']} {href}>{record.name}</a>{:else}<span class={['text-emphasis break-words', cancelled && 'text-fg-muted line-through']}>{record.name}</span>{/if}
						{#if cancelled}<span class="pill bg-raised text-critical">Cancelled</span>{:else if record.status === EVENT_STATUS.rescheduled}<span class="pill bg-raised text-fg-soft">New date</span>{/if}
						{#if confirmed !== null && !cancelled}<span class={['pill', confirmed ? 'bg-raised text-good' : 'bg-well text-fg-muted']}>{confirmed ? 'Confirmed' : 'Waiting for the venue'}</span>{/if}
					</p>
					{#if by}
						<p class="text-small"><a class="entity-link text-fg-soft" href={by.href}>{by.name}</a></p>
					{/if}
					<p class="flex flex-wrap gap-x-1.5 text-small text-fg-muted">
						<span class="font-mono">{showHours(record)}</span>
						{#if place || city}<span aria-hidden="true">·</span><span>{#if place?.href}<a class="entity-link" href={place.href}>{place.name}</a>{:else}{place?.name}{/if}{place && city ? ', ' : ''}{city}</span>{/if}
						{#if item.distanceKm !== undefined}<span aria-hidden="true">·</span><span class="font-mono">{formatDistance(item.distanceKm)}</span>{/if}
					</p>
					{#if (record.tickets && !cancelled) || edit}
						<p class="-ml-2.5 flex flex-wrap items-center gap-1">
							{#if record.tickets && !cancelled}<a class="btn-ghost btn-sm gap-1.5 px-2.5" href={webHref(record.tickets)} rel="noopener noreferrer nofollow" target="_blank">Tickets<ExternalLink class="size-3.5" /></a>{/if}
							{#if edit}<a class="btn-ghost btn-sm gap-1.5 px-2.5" href={edit} aria-label="Edit {record.name}"><Pencil class="size-3.5" />Edit</a>{/if}
						</p>
					{/if}
					{#if edit && edited.includes(item.uri)}
						<p class="notice notice-info mt-1 text-small">Changed in another calendar app. <a class="link" href={edit}>Review it</a> to update this listing.</p>
					{/if}
				</div>
			</li>
		{/each}
	</ol>
{/if}
