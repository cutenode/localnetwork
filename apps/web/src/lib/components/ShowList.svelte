<script lang="ts">
	import { ExternalLink, Pencil } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import { formatDistance } from '#lib/geo.ts';
	import { EVENT_STATUS, isCancelled, showHours, venueDay } from '#lib/shows.ts';
	import type { ShowView } from '#lib/types.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';

	/**
	 * Upcoming shows, soonest (or nearest) first, in every state a listing has: `shows` undefined while they load (as
	 * `rows` placeholders), null when the directory can't answer, empty with `empty`'s title and body and `children`
	 * as its action. Each state is at least a show's height, so one show, none, or the placeholder take the same room.
	 * `project` says whose shows these are, which leaves each show's project off. `editable` gives the viewer's own
	 * shows an edit link, and `edited` marks the ones another app changed since.
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
		shows: ShowView[] | null | undefined;
		project?: string;
		editable?: (show: ShowView) => string | null;
		edited?: string[];
		rows?: number;
		empty: { title: string; body?: string };
		retryHref?: string;
		children?: Snippet;
	} = $props();
</script>

{#snippet note(title: string, body: string | undefined, action: Snippet | undefined)}
	<div class="card flex min-h-26 flex-wrap items-center justify-between gap-x-6 gap-y-3 p-4 sm:min-h-29 sm:px-5">
		<div class="flex flex-col gap-1">
			<p class="m-0 font-semibold">{title}</p>
			{#if body}<p class="m-0 text-small text-fg-muted">{body}</p>{/if}
		</div>
		{#if action}{@render action()}{/if}
	</div>
{/snippet}

{#snippet retry()}<a class="btn-secondary btn-sm" href={retryHref}>Try again</a>{/snippet}

{#if shows === undefined}
	<ol aria-busy="true" aria-label="Loading shows" class="m-0 flex list-none flex-col gap-2.5 p-0">
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
	<ol class="m-0 flex list-none flex-col gap-2.5 p-0">
		{#each shows as show (show.uri)}
			{@const record = show.record}
			{@const edit = editable?.(show)}
			<li class="card flex min-h-26 items-start gap-4 p-4 sm:min-h-29">
				<DateBlock date={venueDay(record.startsAt)} />
				<div class="flex min-w-0 flex-1 flex-col gap-1">
					<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
						<span class={['text-emphasis font-semibold break-words', isCancelled(record) && 'text-fg-muted line-through']}>{record.name}</span>
						{#if isCancelled(record)}<span class="pill bg-raised text-critical">Cancelled</span>{:else if record.status === EVENT_STATUS.rescheduled}<span class="pill bg-raised text-fg-soft">New date</span>{/if}
					</p>
					{#if project !== show.project.uri}
						<p class="m-0 text-small"><a class="link" href="/projects/{show.project.slug}">{show.project.profile.name}</a></p>
					{/if}
					<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
						<span class="font-mono">{showHours(record)}</span>
						<span aria-hidden="true">·</span><span>{[record.venue, record.location.city].filter(Boolean).join(', ')}</span>
						{#if show.distanceKm !== undefined}<span aria-hidden="true">·</span><span class="font-mono">{formatDistance(show.distanceKm)}</span>{/if}
					</p>
					{#if (record.tickets && !isCancelled(record)) || edit}
						<p class="m-0 flex flex-wrap items-center gap-x-4 gap-y-1 text-small">
							{#if record.tickets && !isCancelled(record)}<a class="link inline-flex items-center gap-1" href={record.tickets} rel="noopener noreferrer nofollow" target="_blank">Tickets<ExternalLink class="size-3" /></a>{/if}
							{#if edit}<a class="link inline-flex items-center gap-1" href={edit} aria-label="Edit {record.name}"><Pencil class="size-3" />Edit</a>{/if}
						</p>
					{/if}
					{#if edit && edited.includes(show.uri)}
						<p class="notice notice-info m-0 mt-1 text-small">Changed in another calendar app. <a class="link" href={edit}>Review it</a> to update this listing.</p>
					{/if}
				</div>
			</li>
		{/each}
	</ol>
{/if}
