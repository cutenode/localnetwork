<script lang="ts">
	import { enhance } from '$app/forms';
	import { CalendarDays, Send, Users } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime, formatShowDate } from '#lib/format.ts';
	import { BOOKING_STATUSES, FEE_TIERS, labelFor, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const thread = $derived(data.thread);

	let draft = $state('');
</script>

<svelte:head><title>{thread.title ?? 'Booking'} · Local Network</title></svelte:head>

<div class="container-page grid max-w-6xl gap-6 py-10 lg:grid-cols-[1fr_20rem]">
	<section class="space-y-4">
		<a href="/bookings" class="link text-sm">← All bookings</a>
		<div class="flex flex-wrap items-center gap-3">
			<h1 class="text-3xl">{thread.title ?? 'Booking inquiry'}</h1>
			<StatusPill status={thread.status} />
		</div>
		{#if thread.project}<p class="muted">With <a class="link" href="/projects/{thread.project.slug}">{thread.project.name}</a></p>{/if}
		<FormMessage {form} />
		<ol class="space-y-4">
			{#each data.events as event (event.uri)}
				{@const mine = event.author.did === data.viewer?.did}
				<li class="flex {mine ? 'justify-end' : 'justify-start'}">
					<div class="max-w-xl px-4 py-2 {event.kind === 'status' ? 'border-2 border-dashed border-ansi-dark text-ansi-cyan' : mine ? 'bg-ansi-blue text-ansi-white' : 'card'}">
						<p class="mb-1 text-xs opacity-70"><Handle account={event.author} /> · {formatDateTime(event.createdAt)}</p>
						{#if event.kind === 'inquiry' && event.show}
							<div class="mb-3 grid gap-0.5 border-2 border-dashed border-ansi-cyan px-3 py-2">
								<p class="text-ansi-white">{event.show.title}</p>
								{#if event.show.date}<p class="inline-flex items-center gap-1"><CalendarDays class="size-3.5" />{formatShowDate(event.show.date, true)}</p>{/if}
								{#if event.show.venue}<p>{event.show.venue}{event.show.capacity ? ` · ${event.show.capacity} capacity` : ''}</p>{/if}
								{#if event.show.budget}<p>Budget {event.show.budget.amount.toLocaleString('en')} {event.show.budget.currency}</p>{:else if event.show.budgetTier}<p>{labelFor(FEE_TIERS, event.show.budgetTier)} budget</p>{/if}
								{#if event.show.setType}<p>{labelFor(SET_TYPES, event.show.setType)}{event.show.setMinutes ? `, ${event.show.setMinutes} min` : ''}</p>{/if}
								{#if event.show.genres?.length}<p>{event.show.genres.join(', ')}</p>{/if}
							</div>
						{/if}
						{#if event.kind === 'status'}
							<p>Marked <strong>{event.status}</strong>{event.text ? `: ${event.text}` : ''}</p>
						{:else}
							<p class="whitespace-pre-line">{event.text}</p>
						{/if}
					</div>
				</li>
			{/each}
		</ol>
		<form method="POST" action="?/message" class="card flex items-end gap-2 p-3" use:enhance={() => async ({ update, result }) => { await update(); if (result.type === 'success') draft = ''; }}>
			<label class="sr-only" for="text">Message</label>
			<textarea id="text" name="text" class="input min-h-12 flex-1 resize-y" placeholder="Write a message…" bind:value={draft} required maxlength="2000"></textarea>
			<button class="btn-accent" aria-label="Send"><Send class="size-4" /></button>
		</form>
	</section>
	<aside class="space-y-4">
		<section class="card card-pad space-y-3">
			<h2 class="section-title flex items-center gap-2"><Users class="size-4" />In this thread</h2>
			<ul class="space-y-1 text-sm">
				{#each thread.participants as participant (participant.did)}<li class="flex justify-between"><Handle account={participant} /><span class="muted">{participant.role}</span></li>{/each}
			</ul>
		</section>
		<form method="POST" action="?/status" class="card card-pad space-y-3" use:enhance>
			<h2 class="section-title">Status</h2>
			<select name="status" class="input" aria-label="Status">
				{#each BOOKING_STATUSES as option (option.value)}<option value={option.value} selected={thread.status === option.value}>{option.label}</option>{/each}
			</select>
			<input name="note" class="input" placeholder="Note (optional)" maxlength="500" />
			<button class="btn-secondary w-full">Update status</button>
		</form>
		<p class="mono muted px-1">{thread.space}</p>
	</aside>
</div>
