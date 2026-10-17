<script lang="ts">
	import { enhance } from '$app/forms';
	import { ArrowLeft, Megaphone, Send } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
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
			['Venue', show?.venue],
			['Capacity', show?.capacity?.toLocaleString('en')],
			['Budget', budget],
			['Set', set]
		].filter((row): row is [string, string] => Boolean(row[1]))
	);

	let draft = $state('');

	// A confirmed booking is a show the project's managers can announce, filled in from these details.
	const announce = $derived(
		thread.status === 'confirmed' && thread.project && thread.participants.some((p) => p.did === data.viewer?.did && p.role === 'manager')
			? `/projects/${thread.project.slug}/shows/new?booking=${thread.skey}`
			: null
	);
</script>

<svelte:head><title>{thread.title ?? 'Booking'} · Local Network</title></svelte:head>

<div class="container-page flex max-w-3xl flex-col gap-6 py-10 sm:py-14">
	<a href="/bookings" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />All bookings</a>

	<header class="flex flex-col gap-3">
		<Eyebrow>Booking thread</Eyebrow>
		<h1 class="m-0 text-title font-semibold">{thread.title ?? 'Booking inquiry'}</h1>
		<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
			{#if thread.project}<span>With <a class="link" href="/projects/{thread.project.slug}">{thread.project.name}</a></span><span aria-hidden="true">·</span>{/if}
			{#each thread.participants as participant, index (participant.did)}<span><Handle account={participant} /> ({participant.role}){index < thread.participants.length - 1 ? ',' : ''}</span>{/each}
		</p>
	</header>

	<FormMessage {form} />

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
		<form method="POST" action="?/status" class="flex flex-col gap-3 border-t border-line bg-sunken p-5" use:enhance>
			<div class="flex flex-wrap items-center gap-2.5">
				<h2 class="m-0 text-small font-medium text-fg-soft">Status</h2>
				<StatusPill status={thread.status} />
				{#if announce}<a class="btn-secondary btn-sm ml-auto" href={announce}><Megaphone class="size-4" />Announce this show</a>{/if}
			</div>
			<div class="flex flex-col gap-2 sm:flex-row">
				<select name="status" class="input sm:w-44" aria-label="Status">
					{#each BOOKING_STATUSES as option (option.value)}<option value={option.value} selected={thread.status === option.value}>{option.label}</option>{/each}
				</select>
				<input name="note" class="input sm:flex-1" placeholder="Note (optional)" aria-label="Note" maxlength="500" />
				<button class="btn-secondary">Update status</button>
			</div>
		</form>
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
		<form method="POST" action="?/message" class="card flex items-end gap-2 p-2" use:enhance={() => async ({ update, result }) => { await update(); if (result.type === 'success') draft = ''; }}>
			<label class="sr-only" for="text">Message</label>
			<textarea id="text" name="text" class="input max-h-60 min-h-11 flex-1 field-sizing-content resize-none border-transparent bg-transparent py-2.5" placeholder="Write a message…" bind:value={draft} required maxlength="2000"></textarea>
			<button class="btn-primary px-3.5" aria-label="Send"><Send class="size-4" /><span class="max-sm:hidden">Send</span></button>
		</form>
	</section>

	<p class="m-0 font-mono text-caption break-all text-fg-subtle">{thread.space}</p>
</div>
