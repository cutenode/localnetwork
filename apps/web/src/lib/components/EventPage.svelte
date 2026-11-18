<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import { Check, Copy, ExternalLink, MapPin } from '@lucide/svelte';
	import { enhance, type SubmitFunction } from '$app/forms';
	import type { EventPageView } from '#lib/events.ts';
	import { asShowDate, formatShowDate } from '#lib/format.ts';
	import { EVENT_STATUS, showHours, venueClock, venueDay } from '#lib/shows.ts';
	import { webHref } from '#lib/web-url.ts';
	import FormMessage from './FormMessage.svelte';
	import VenuePicker from './VenuePicker.svelte';
	import EditMode from './edit/EditMode.svelte';
	import InlineText from './edit/InlineText.svelte';
	import LineupFields from './form/LineupFields.svelte';

	/**
	 * An event's page, built in place like its party's: its name, night, venue, party, lineup, tickets and what it's
	 * about, and whether the venue has confirmed it. In edit mode the same layout has each part as its field. Times are
	 * on the venue's clock, or until there's a venue, on a time zone picked with them; a save that would move them onto
	 * another clock comes back asking whether to keep them as entered or at the same moments (`view.choice`). An event
	 * being made has no `event` yet and draws every field empty. Its people with events.manage also get, outside edit
	 * mode, a private event's `link` to share or reset, and a way to cancel it.
	 */
	let {
		event = null,
		party,
		link = null,
		timeZone = 'UTC',
		timeZones = [],
		...edit
	}: Omit<ComponentProps<typeof EditMode>, 'children' | 'name' | 'labels' | 'missing'> & {
		event?: EventPageView | null;
		/** The party it's under, with its page. */
		party: { name: string; href: string };
		/** A private event's whole link, token and all, for its people with events.manage. */
		link?: string | null;
		/** The time zone an event being made starts on, until it has a venue. */
		timeZone?: string;
		/** The time zones a draft without a venue can be on, sent while editing. */
		timeZones?: string[];
	} = $props();

	const id = $props.id();

	const name = $derived(event?.name ?? 'New event');

	// Picked here while editing; starts from the saved one, and from it again once a save comes back.
	let venue = $derived(event?.venue ?? null);

	/** The clock its times are on: its venue's, or its own while it has none. */
	const clock = $derived(event ? (event.timeZone ?? 'UTC') : timeZone);

	const zones = $derived(timeZones.includes(clock) ? timeZones : [clock, ...timeZones]);

	const isCancelled = $derived(event?.status === EVENT_STATUS.cancelled);

	const isPublished = $derived(edit.status !== 'draft');

	const manages = $derived(!!edit.can?.includes('events.manage'));

	const missing = $derived([...(event ? [] : ['a name']), ...(event?.venue ? [] : ['a venue']), ...(event?.startsAt ? [] : ['a date'])]);

	const labels = { name: 'Name', description: 'About', date: 'Date', start: 'Starts', end: 'Ends', tickets: 'Ticket link', audience: 'Who can see it' };

	const AUDIENCES = [
		{ value: 'public', label: 'Public', hint: 'Anyone can find it once the venue lists it' },
		{ value: 'private', label: 'Private', hint: 'Only people with its link can open it' }
	];

	let copied = $state(false);

	/** How cancelling or resetting the link is sent: SvelteKit's enhance, unless a harness answers for the server. */
	const send = $derived(edit.enhance ?? enhance);

	/** Why cancelling or resetting the link didn't go through, which stays until the next try. */
	let problem = $state('');

	async function copyLink() {
		if (!link) return;

		try {
			await navigator.clipboard.writeText(link);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}

	/** Asks `question` before sending, since neither can be taken back, and keeps a refusal beside the button. */
	const confirmed =
		(question: string): SubmitFunction<undefined, { error: string }> =>
		({ cancel }) => {
			if (!confirm(question)) return cancel();

			return async ({ result, update }) => {
				problem = result.type === 'failure' ? (result.data?.error ?? '') : '';

				if (result.type !== 'failure') await update();
			};
		};
</script>

<EditMode {...edit} {name} {labels} {missing}>
	{#snippet children(view)}
		{#if edit.status === 'draft' && !view.visitor}<div class="container-page pt-6"><p class="m-0 notice notice-info">This is a draft. Only the party's people can see it.</p></div>{/if}
		<div class="border-b border-line">
			<div class="container-page flex flex-col gap-5 py-10 sm:flex-row sm:items-end sm:py-14">
				<div class="flex min-w-0 flex-1 flex-col gap-2">
					<p class="m-0 text-small"><a class="entity-link text-fg-soft" href={party.href}>{party.name}</a></p>
					<h1 class={['m-0 text-title font-semibold', isCancelled && !view.editing && 'text-fg-muted line-through']}>
						{#if view.editing}<InlineText name="name" label="Name" required placeholder="Event name" value={view.value('name', event?.name ?? '')} error={view.error('name')} readonly={view.busy} class="text-title font-semibold" />{:else}{event?.name}{/if}
					</h1>
					{#if event && !view.editing}
						{#if event.startsAt}
							<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
								<time datetime={event.startsAt}>{formatShowDate(asShowDate(event.startsAt))}</time>
								<span aria-hidden="true">·</span><span class="font-mono">{showHours({ startsAt: event.startsAt, endsAt: event.endsAt })}</span>
								{#if !event.venue}<span aria-hidden="true">·</span><span>{clock}</span>{/if}
							</p>
						{/if}
						{#if event.venue}
							<p class="m-0 inline-flex items-center gap-1.5 text-small text-fg-muted"><MapPin aria-hidden="true" class="size-4" />{[event.venue.name, event.venue.locality ?? event.venue.country].join(', ')}</p>
						{/if}
						<p class="m-0 flex flex-wrap gap-2">
							{#if isCancelled}
								<span class="pill bg-raised text-critical">Cancelled</span>
							{:else}
								{#if event.status === EVENT_STATUS.rescheduled}<span class="pill bg-raised text-fg-soft">New date</span>{/if}
								{#if event.venue && isPublished}<span class={['pill', event.confirmed ? 'bg-raised text-good' : 'bg-well text-fg-muted']}>{event.confirmed ? 'Confirmed' : 'Waiting for the venue'}</span>{/if}
							{/if}
							{#if event.audience === 'private' && !view.visitor}<span class="pill bg-well text-fg-muted">Private</span>{/if}
						</p>
					{/if}
				</div>
				{#if event?.tickets && !isCancelled && !view.editing}
					<a class="btn-primary self-start sm:self-auto" href={webHref(event.tickets)} rel="noopener noreferrer nofollow" target="_blank">Tickets<ExternalLink aria-hidden="true" class="size-4" /></a>
				{/if}
			</div>
		</div>

		<div class="container-page flex flex-wrap gap-x-10 gap-y-8 py-10">
			<div class="flex min-w-0 flex-[999_1_34rem] flex-col gap-8">
				{#if view.editing}
					<InlineText name="description" label="About" multiline placeholder="What the night is about" value={view.value('description', event?.description ?? '')} error={view.error('description')} readonly={view.busy} class="text-body text-fg-soft" />
					<section aria-labelledby="{id}-where" class="card flex flex-col gap-3 p-5">
						<h2 id="{id}-where" class="m-0 eyebrow text-fg-subtle">Where</h2>
						<VenuePicker id="{id}-venue" bind:value={venue} />
					</section>
					<section aria-labelledby="{id}-when" class="card flex flex-col gap-3 p-5">
						<h2 id="{id}-when" class="m-0 eyebrow text-fg-subtle">When</h2>
						{#if view.choice}
							<div class="notice notice-info flex flex-col gap-3">
								<p class="m-0">{view.choice.question}</p>
								<!-- A publish that was asked this publishes once it's answered. -->
								{#if view.value('publish', '') === 'on'}<input type="hidden" name="publish" value="on" />{/if}
								<div class="flex flex-wrap gap-2">
									{#each view.choice.options as option (option.value)}<button type="submit" name={view.choice.name} value={option.value} class="btn-secondary btn-sm" disabled={view.busy}>{option.label}</button>{/each}
								</div>
							</div>
						{/if}
						<div class="grid gap-3 sm:grid-cols-3">
							<div><label class="label" for="{id}-date">Date</label><input id="{id}-date" type="date" name="date" class="input" value={view.value('date', event?.startsAt ? venueDay(event.startsAt) : '')} readonly={view.busy} /></div>
							<div><label class="label" for="{id}-start">Starts</label><input id="{id}-start" type="time" name="start" class="input" value={view.value('start', event?.startsAt ? venueClock(event.startsAt) : '')} readonly={view.busy} /></div>
							<div><label class="label" for="{id}-end">Ends</label><input id="{id}-end" type="time" name="end" class="input" value={view.value('end', event?.endsAt ? venueClock(event.endsAt) : '')} readonly={view.busy} /></div>
						</div>
						{#if !venue}
							<div>
								<label class="label" for="{id}-zone">Time zone</label>
								<select id="{id}-zone" name="timeZone" class="input" disabled={view.busy}>
									{#each zones as zone (zone)}<option value={zone} selected={zone === view.value('timeZone', clock)}>{zone}</option>{/each}
								</select>
							</div>
						{/if}
						<p class="m-0 hint">{venue ? "On the venue's clock. " : ''}An end before the start is the next morning.</p>
					</section>
					<section aria-labelledby="{id}-lineup" class="card flex flex-col gap-3 p-5">
						<h2 id="{id}-lineup" class="m-0 eyebrow text-fg-subtle">Lineup</h2>
						<LineupFields saved={event?.lineup} error={view.error('performer')} readonly={view.busy} />
					</section>
					<section aria-labelledby="{id}-tickets" class="card flex flex-col gap-3 p-5">
						<h2 id="{id}-tickets" class="m-0 eyebrow text-fg-subtle">Tickets</h2>
						<input type="url" name="tickets" class="input" maxlength="2000" placeholder="https://" aria-label="Ticket link" value={view.value('tickets', event?.tickets ?? '')} readonly={view.busy} />
					</section>
					<section aria-labelledby="{id}-audience" class="card flex flex-col gap-3 p-5">
						<h2 id="{id}-audience" class="m-0 eyebrow text-fg-subtle">Who can see it</h2>
						<fieldset class="flex flex-col gap-3">
							<legend class="sr-only">Who can see it</legend>
							{#each AUDIENCES as option (option.value)}
								<label class="flex items-start gap-2.5">
									<input type="radio" class="checkbox mt-1 rounded-full" name="audience" value={option.value} checked={view.value('audience', event?.audience ?? 'public') === option.value} />
									<span class="flex flex-col">
										<span class="text-body text-fg">{option.label}</span>
										<span class="text-small text-fg-muted">{option.hint}</span>
									</span>
								</label>
							{/each}
						</fieldset>
					</section>
				{:else if event}
					{#if event.description}<p class="m-0 text-body whitespace-pre-line text-fg-soft">{event.description}</p>{/if}
					{#if event.lineup.length}
						<section aria-labelledby="{id}-lineup" class="flex flex-col gap-3">
							<h2 id="{id}-lineup" class="m-0 eyebrow text-fg-subtle">Lineup</h2>
							<ul class="m-0 flex list-none flex-col divide-y divide-line p-0">
								{#each event.lineup as entry, index (index)}
									<li class="flex items-baseline justify-between gap-3 py-2.5">
										{#if entry.href}<a class="link text-emphasis font-medium" href={entry.href}>{entry.name}</a>{:else}<span class="text-emphasis font-medium">{entry.name}</span>{/if}
										{#if entry.start}<span class="shrink-0 font-mono text-small text-fg-muted">{entry.end ? `${entry.start} to ${entry.end}` : entry.start}</span>{/if}
									</li>
								{/each}
							</ul>
						</section>
					{/if}
				{/if}
			</div>

			{#if manages && !view.editing && !view.visitor && (link || (isPublished && !isCancelled))}
				<aside class="flex min-w-0 flex-[1_1_18rem] flex-col gap-6">
					{#if link}
						<section aria-labelledby="{id}-link" class="card flex flex-col gap-3 p-5">
							<h2 id="{id}-link" class="m-0 eyebrow text-fg-subtle">Private link</h2>
							<input class="input font-mono text-small" readonly value={link} aria-label="Private link" onfocus={(focused) => focused.currentTarget.select()} />
							<p class="m-0 hint">Anyone with this link can open it.</p>
							<div class="flex flex-wrap gap-2">
								<button type="button" class="btn-secondary btn-sm" onclick={copyLink}>{#if copied}<Check class="size-4" />Copied{:else}<Copy class="size-4" />Copy link{/if}</button>
								<form method="POST" action="?/resetLink" use:send={confirmed('Reset the link? The old one stops working.')}>
									<button class="btn-ghost btn-sm">Reset link</button>
								</form>
							</div>
						</section>
					{/if}
					{#if isPublished && !isCancelled}
						<form method="POST" action="?/cancel" use:send={confirmed(`Cancel ${name}? It stays listed, marked Cancelled.`)}>
							<input type="hidden" name="cid" value={edit.version} />
							<button class="btn-ghost btn-sm text-critical">Cancel event</button>
						</form>
					{/if}
					<FormMessage form={{ error: problem }} />
				</aside>
			{/if}
		</div>
	{/snippet}
</EditMode>
