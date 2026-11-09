<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { ArrowLeft, ChevronDown, Megaphone, Send } from '@lucide/svelte';
	import { tick } from 'svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { type BookingAction, bookingActions } from '#lib/bookings.ts';
	import { formatDateTime, formatShowDate } from '#lib/format.ts';
	import { BOOKING_STATUSES, FEE_TIERS, labelFor, SET_TYPES } from '#lib/taxonomy/options.ts';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const thread = $derived(data.thread);

	// The inquiry carries the show; it goes up front instead of inside the first message.
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

	const role = $derived(thread.participants.find((p) => p.did === data.viewer?.did)?.role);

	// The promoter can confirm an open inquiry only once a manager has written in the thread, as the server checks.
	const answered = $derived(data.events.some((event) => thread.participants.some((p) => p.did === event.author.did && p.role === 'manager')));

	const actions = $derived(role ? bookingActions(role, thread.status, answered) : []);

	let changing = $state(false);

	/** The settling change waiting for its confirm. */
	let pending = $state<BookingAction>();

	let submitting = $state(false);

	/** Which form sent last, so its error shows by it; none without JavaScript, when the error shows up top. */
	let sent = $state<'status' | 'message'>();

	const formError = $derived(form && 'error' in form ? form.error : undefined);

	/** Read out once a change lands; the pill and the conversation show it. */
	let announced = $state('');

	let toggle = $state<HTMLButtonElement>();

	// A confirmed booking is a show the project's managers can announce, filled in from these details.
	const announce = $derived(thread.status === 'confirmed' && thread.project && role === 'manager' ? `/dashboard/projects/${thread.project.slug}/shows/new?booking=${thread.skey}` : null);
</script>

<svelte:head><title>{thread.title ?? 'Booking'} · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-6">
	<a href="/dashboard/bookings" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />All bookings</a>

	<header class="flex flex-col gap-3">
		<Eyebrow>Booking thread</Eyebrow>
		<h1 class="m-0 text-title font-semibold">{thread.title ?? 'Booking inquiry'}</h1>
		<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
			{#if thread.project}<span>With <a class="link" href="/projects/{thread.project.slug}">{thread.project.name}</a></span><span aria-hidden="true">·</span>{/if}
			{#each thread.participants as participant, index (participant.did)}<span><Handle account={participant} /> ({participant.role}){index < thread.participants.length - 1 ? ',' : ''}</span>{/each}
		</p>
	</header>

	<!-- Without JavaScript, errors come back here; with it, each shows by its own control. -->
	{#if !sent}<FormMessage {form} />{/if}

	<Card as="section" padding="none" class="overflow-hidden">
		<div class="flex flex-col gap-4 p-5">
			<h2 class="m-0 text-emphasis font-semibold">Show details</h2>
			{#if details.length}
				<dl class="m-0 grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3">
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
		<div class="flex flex-col gap-4 border-t border-line bg-sunken p-5">
			<div class="flex flex-wrap items-center gap-2.5">
				<h2 class="m-0 text-small font-medium text-fg-soft">Status</h2>
				<StatusPill status={thread.status} />
				<p role="status" class="sr-only">{announced}</p>
				<div class="ml-auto flex flex-wrap gap-2">
					{#if announce}<a class="btn-secondary btn-sm" href={announce}><Megaphone class="size-4" />Announce this show</a>{/if}
					{#if actions.length}
						<button bind:this={toggle} type="button" class="btn-secondary btn-sm" aria-expanded={changing} aria-controls="change-status" onclick={() => ((changing = !changing), (pending = undefined), (sent = undefined))}>
							Change status<ChevronDown class={['size-4 transition-transform', changing && 'rotate-180']} aria-hidden="true" />
						</button>
					{/if}
				</div>
			</div>
			{#if changing && actions.length}
				<form
					id="change-status"
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
							changing = false;
							pending = undefined;
							await tick();
							toggle?.focus();
						};
					}}
				>
					<input name="note" class="input" placeholder="Note for the thread (optional)" aria-label="Note" maxlength="500" />
					{#if pending?.settles}
						<p class="m-0 text-small text-fg">{pending.settles.ask}</p>
						<div class="flex flex-wrap gap-2">
							<button name="status" value={pending.status} class={[pending.status === 'confirmed' ? 'btn-primary' : 'btn-danger', 'btn-sm']} disabled={submitting}>{pending.label}</button>
							<button type="button" class="btn-ghost btn-sm" onclick={() => (pending = undefined)} disabled={submitting}>Back</button>
						</div>
					{:else}
						<div class="flex flex-wrap gap-2">
							{#each actions as action (action.label)}
								{#if action.settles}
									<button type="button" class={[action.status === 'confirmed' ? 'btn-primary' : 'btn-secondary', 'btn-sm']} onclick={() => (pending = action)}>{action.label}</button>
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

	<section aria-labelledby="conversation" class="flex flex-col gap-4">
		<h2 id="conversation" class="m-0 text-heading font-semibold">Conversation</h2>
		<ol class="m-0 flex list-none flex-col gap-4 p-0">
			{#each data.events as event (event.uri)}
				{@const mine = event.author.did === data.viewer?.did}
				{#if event.kind === 'status'}
					<li class="text-center text-caption text-fg-subtle">
						<Handle account={event.author} class="text-fg-muted" /> marked it <span class="text-fg-soft">{event.status}</span>{event.text ? `: ${event.text}` : ''} · {formatDateTime(event.createdAt)}
					</li>
				{:else}
					<li class={['flex flex-col gap-1', mine ? 'items-end' : 'items-start']}>
						<p class="m-0 flex gap-1.5 px-1 text-caption text-fg-subtle">{#if !mine}<Handle account={event.author} class="text-fg-muted" /><span aria-hidden="true">·</span>{/if}<span>{formatDateTime(event.createdAt)}</span></p>
						<div class={['max-w-[85%] rounded-2xl px-4 py-2.5 sm:max-w-[75%]', mine ? 'rounded-br-sm bg-link-wash' : 'rounded-bl-sm border border-line bg-surface']}>
							<p class="m-0 whitespace-pre-line text-fg">{event.text}</p>
						</div>
					</li>
				{/if}
			{/each}
		</ol>
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
			<button class="btn-primary px-3.5" aria-label="Send" disabled={sending}><Send class="size-4" /><span class="max-sm:hidden">Send</span></button>
		</form>
	</section>
</div>
