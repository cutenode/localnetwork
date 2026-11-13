<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { ArrowLeft, Megaphone, Send } from '@lucide/svelte';
	import { tick } from 'svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { type BookingAction, bookingActions, bookingGroup, datePassed } from '#lib/bookings.ts';
	import { accountLabel, formatDateTime, formatShowDate, nightLabel } from '#lib/format.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import { BOOKING_STATUSES, FEE_TIERS, labelFor, SET_TYPES } from '#lib/taxonomy/options.ts';
	import Card from '#lib/ui/Card.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import type { BookingEventView } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// Undefined until the directory answers, and null when it can't say.
	const act = resolved(() => data.project, undefined);

	const thread = $derived({ ...data.thread, project: act.value ?? null });

	const me = $derived(data.viewer?.did ?? '');

	// The inquiry carries the show; it goes in the rail instead of inside the first message.
	const show = $derived(data.events.find((event) => event.kind === 'inquiry' && event.show)?.show);

	const budget = $derived(show?.budget ? `${show.budget.amount.toLocaleString('en')} ${show.budget.currency}` : show?.budgetTier ? labelFor(FEE_TIERS, show.budgetTier) : null);

	const set = $derived(show?.setType ? `${labelFor(SET_TYPES, show.setType)}${show.setMinutes ? `, ${show.setMinutes} min` : ''}` : show?.setMinutes ? `${show.setMinutes} min` : null);

	const details = $derived(
		[
			['Date', show?.date ? formatShowDate(show.date, true) : null],
			['Venue', [show?.venue, show?.location?.city].filter(Boolean).join(', ')],
			['Capacity', show?.capacity?.toLocaleString('en')],
			['Budget', budget],
			['Set', set]
		].filter((row): row is [string, string] => Boolean(row[1]))
	);

	let draft = $state('');

	let sending = $state(false);

	const roleOf = (did: string) => thread.participants.find((p) => p.did === did)?.role;

	const role = $derived(roleOf(me));

	const group = $derived(bookingGroup(thread, me));

	// The promoter can confirm an open inquiry only once a manager has written in the thread, as the server checks.
	const answered = $derived(data.events.some((event) => roleOf(event.author.did) === 'manager'));

	const actions = $derived(role ? bookingActions(role, thread.status, answered) : []);

	// Only a confirmed booking can be cancelled, so a cancelled one that never was is an inquiry someone took back.
	const withdrawn = $derived(thread.status === 'cancelled' && !data.events.some((event) => event.status === 'confirmed'));

	const isSettled = $derived(thread.status === 'declined' || thread.status === 'cancelled');

	/** The settling change waiting for its confirm. */
	let pending = $state<BookingAction>();

	let submitting = $state(false);

	/** Which form sent last, so its error shows by it; none without JavaScript, when the error shows up top. */
	let sent = $state<'status' | 'message'>();

	const formError = $derived(form && 'error' in form ? form.error : undefined);

	/** Read out once a change lands; the pill and the conversation show it. */
	let announced = $state('');

	/** Where focus goes when the button it was on leaves. */
	let statusHeading = $state<HTMLElement>();

	const back = async () => {
		await tick();
		statusHeading?.focus();
	};

	// A confirmed booking is a show the act's managers can announce, filled in from these details.
	const canAnnounce = $derived(thread.status === 'confirmed' && role === 'manager');

	const ghost = 'rounded-md bg-well motion-safe:animate-pulse';
</script>

<svelte:head><title>{thread.title ?? 'Booking'} · Local Network</title></svelte:head>

<!-- The name fills in when the directory answers. `fallback` stands in when it doesn't. -->
{#snippet actName(fallback: string)}
	{#if thread.project}{thread.project.name}{:else if act.value === undefined}<span class={['inline-block h-3 w-20 align-baseline', ghost]}></span>{:else}{fallback}{/if}
{/snippet}

<!-- The act speaks as itself, so a promoter reads its name and not whichever of its people wrote (docs/adr/0004). -->
{#snippet author(event: BookingEventView)}
	{#if role === 'promoter' && roleOf(event.author.did) === 'manager'}
		<span class="text-fg-muted">{@render actName('The act')}</span>
	{:else}
		<Handle account={event.author} class="text-fg-muted" />
	{/if}
{/snippet}

<div class="flex max-w-5xl flex-col gap-6 sm:gap-8">
	<header class="flex flex-col gap-3">
		<a href="/dashboard/bookings" class="inline-flex min-h-6 items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" aria-hidden="true" />Bookings</a>
		<div class="flex flex-col gap-1">
			<div class="flex min-w-0 flex-col gap-1">
				<h1 class="m-0 text-heading font-semibold">{thread.title ?? 'Booking inquiry'}</h1>
				<p class="m-0 min-h-lh text-body text-fg-muted">
					{#if role === 'manager'}
						{#if thread.project || act.value === undefined}{@render actName('')} and{/if}
						<Handle account={thread.promoter} class="text-fg-muted" />
					{:else}
						You and {@render actName('the act')}
					{/if}
					{#if thread.showDate}· {nightLabel(thread.showDate)}{/if}
				</p>
			</div>
			{#if group === 'reply'}
				<p class="m-0 text-small font-semibold text-warning">Your turn</p>
			{:else if group === 'active'}
				<p class="m-0 text-small text-fg-muted">
					Waiting on
					{#if role === 'manager'}{accountLabel(thread.promoter)}{:else}{@render actName('the act')}{/if}
				</p>
			{/if}
		</div>
	</header>

	<!-- Without JavaScript, errors come back here; with it, each shows by its own control. -->
	{#if !sent}<FormMessage {form} />{/if}

	<div class="grid grid-cols-[minmax(0,1fr)] gap-6 lg:grid-cols-[minmax(0,1fr)_17rem] lg:gap-8 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<aside aria-label="Terms and status" class="flex flex-col gap-3 lg:sticky lg:top-6 lg:col-start-2 lg:row-start-1 lg:self-start">
			<Card as="section" padding="none" class="overflow-hidden">
				<div class="flex flex-col gap-4 p-5">
					<h2 class="m-0 text-emphasis font-semibold">Show details</h2>
					{#if details.length}
						<dl class="m-0 grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3 lg:grid-cols-1">
							{#each details as [label, value] (label)}
								<div class="flex min-w-0 flex-col gap-0.5">
									<dt class="text-caption text-fg-subtle">{label}</dt>
									<dd class="m-0 text-small text-fg">{value}</dd>
								</div>
							{/each}
						</dl>
					{:else}
						<p class="m-0 text-small text-fg-muted">No details yet.</p>
					{/if}
					{#if show?.genres?.length}
						<div class="flex flex-wrap gap-1.5">{#each show.genres as genre (genre)}<Tag>{genre}</Tag>{/each}</div>
					{/if}
				</div>
				<div class="flex flex-col gap-3 border-t border-line bg-sunken p-5">
					<div class="flex flex-wrap items-center gap-2">
						<h2 bind:this={statusHeading} tabindex="-1" class="m-0 text-small font-medium text-fg-soft outline-none">Status</h2>
						<StatusPill status={withdrawn ? 'withdrawn' : thread.status} />
						{#if !isSettled && datePassed(thread)}<StatusPill status="date_passed" />{/if}
						<p role="status" class="sr-only">{announced}</p>
					</div>
					{#if canAnnounce}
						{#if thread.project}
							<a class="btn-secondary btn-sm" href="/dashboard/projects/{thread.project.slug}/shows/new?booking={thread.skey}"><Megaphone class="size-4" aria-hidden="true" />Announce this show</a>
						{:else if act.value === undefined}
							<span class={['h-9', ghost]}></span>
						{:else}
							<p class="m-0 text-small text-fg-muted">Try again in a moment to announce this show.</p>
						{/if}
					{/if}
					{#if actions.length}
						<form
							method="POST"
							action="?/status"
							class="flex flex-col gap-3"
							use:enhance={() => {
								submitting = true;
								sent = 'status';

								return async ({ result, update }) => {
									await update();

									// A refused change usually means the other side moved first: reload to show where the thread is now.
									if (result.type === 'failure') {
										await invalidateAll();
										pending = undefined;
									}

									submitting = false;

									if (result.type !== 'success') return;
									announced = `Marked ${labelFor(BOOKING_STATUSES, thread.status).toLowerCase()}`;
									pending = undefined;
									await back();
								};
							}}
						>
							{#if pending?.settles}
								<input name="note" class="input" placeholder="Note for the thread (optional)" aria-label="Note" maxlength="500" />
								<!-- The question takes focus from the button that went, so it's read out and the next tab lands on the choice. -->
								<p tabindex="-1" class="m-0 text-small text-fg outline-none" {@attach (element) => element.focus()}>{pending.settles.ask}</p>
								<div class="flex flex-wrap gap-2 lg:flex-col">
									<button name="status" value={pending.status} class={[pending.status === 'confirmed' ? 'btn-primary' : 'btn-danger', 'btn-sm']} disabled={submitting}>{pending.label}</button>
									<button
										type="button"
										class="btn-ghost btn-sm"
										onclick={() => {
											pending = undefined;
											void back();
										}}
										disabled={submitting}>Back</button
									>
								</div>
							{:else}
								<div class="flex flex-wrap gap-2 lg:flex-col">
									{#each actions as action (action.label)}
										{#if action.settles}
											<button type="button" class={[action.status === 'confirmed' ? 'btn-secondary' : 'btn-ghost', 'btn-sm']} onclick={() => (pending = action)}>{action.label}</button>
										{:else}
											<button name="status" value={action.status} class="btn-secondary btn-sm" disabled={submitting}>{action.label}</button>
										{/if}
									{/each}
								</div>
							{/if}
						</form>
					{/if}
					{#if sent === 'status' && formError}<p role="alert" class="notice notice-error m-0">{formError}</p>{/if}
				</div>
			</Card>
			<p class="m-0 px-1 text-caption text-fg-subtle">Only the promoter and the act's managers can read this.</p>
		</aside>

		<section aria-labelledby="conversation" class="flex min-w-0 flex-col gap-4 lg:col-start-1 lg:row-start-1">
			<h2 id="conversation" class="m-0 text-emphasis font-semibold lg:sr-only">Conversation</h2>
			{#if data.events.length}
				<ol class="m-0 flex list-none flex-col gap-4 p-0">
					{#each data.events as event (event.uri)}
						{@const mine = event.author.did === me}
						{#if event.kind === 'status'}
							<li class="text-center text-caption text-fg-subtle">
								{#if mine}<span class="text-fg-muted">You</span>{:else}{@render author(event)}{/if}
								marked it <span class="text-fg-soft">{withdrawn && event.status === 'cancelled' ? 'withdrawn' : event.status}</span>{event.text ? `: ${event.text}` : ''} · {formatDateTime(event.createdAt)}
							</li>
						{:else}
							<li class={['flex flex-col gap-1', mine ? 'items-end' : 'items-start']}>
								<p class="m-0 flex gap-1.5 px-1 text-caption text-fg-subtle">{#if !mine}{@render author(event)}<span aria-hidden="true">·</span>{/if}<span>{formatDateTime(event.createdAt)}</span></p>
								<div class={['max-w-[85%] rounded-2xl px-4 py-2.5 sm:max-w-[75%]', mine ? 'rounded-br-sm bg-link-wash' : 'rounded-bl-sm border border-line bg-surface']}>
									<p class="m-0 whitespace-pre-line text-fg">{event.text}</p>
								</div>
							</li>
						{/if}
					{/each}
				</ol>
			{:else}
				<p class="m-0 text-small text-fg-muted">No messages yet.</p>
			{/if}
			{#if sent === 'message' && formError}<p role="alert" class="notice notice-error m-0">{formError}</p>{/if}
			<form
				method="POST"
				action="?/message"
				class="card flex items-end gap-2 p-2"
				use:enhance={() => {
					sent = 'message';
					sending = true;

					return async ({ update, result }) => {
						await update();
						sending = false;

						if (result.type === 'success') draft = '';
					};
				}}
			>
				<label class="sr-only" for="text">Message</label>
				<textarea id="text" name="text" class="input max-h-60 min-h-11 flex-1 field-sizing-content resize-none border-transparent bg-transparent py-2.5" placeholder="Write a message…" bind:value={draft} required maxlength="2000"></textarea>
				<button class="btn-primary px-3.5" aria-label="Send" disabled={sending}><Send class="size-4" aria-hidden="true" /><span class="max-sm:hidden">Send</span></button>
			</form>
		</section>
	</div>
</div>
