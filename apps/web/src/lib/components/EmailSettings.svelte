<script module lang="ts">
	import type { NotificationTopic, PausedReason } from '#lib/types.ts';

	const PAUSED: Partial<Record<PausedReason, (address: string) => string>> = {
		email_unverified: (address) => `We emailed a link to ${address}. Open it within a day to start getting email there.`,
		email_unconfirmed: (address) => `Your account's host hasn't confirmed ${address} yet. Confirm it there, then check again.`,
		address_rejected: (address) => `Email to ${address} didn't go through. Check the address with your account's host, then try again.`
	};
</script>

<script lang="ts">
	import { Mail } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import type { ChannelView } from '#lib/types.ts';
	import ChannelTopics from './ChannelTopics.svelte';
	import FormMessage from './FormMessage.svelte';

	/**
	 * Someone's email setting for their inbox. `available` is false when the site can't send email. `returned` says how
	 * asking their account's host for the address just went, when it didn't go through. Turning email on (or checking
	 * again) is a full page POST: it goes on to their account's host, which client-side navigation can't follow.
	 */
	let {
		available,
		channel,
		returned,
		topics,
		form
	}: {
		available: boolean;
		channel: ChannelView | null;
		returned?: 'declined' | 'limited' | null;
		topics: NotificationTopic[];
		form?: { error?: string; success?: string } | null;
	} = $props();

	const id = $props.id();

	const address = $derived(channel?.address ?? 'your email address');

	const paused = $derived(channel?.status === 'paused' ? (PAUSED[channel.pausedReason ?? 'email_unconfirmed']?.(address) ?? 'Email is paused.') : null);

	const status = $derived(
		!available
			? 'Not set up on this site.'
			: !channel
				? 'Get your inbox by email too.'
				: channel.pausedReason === 'email_unverified'
					? 'Waiting for you to confirm your address.'
					: paused
						? 'Paused.'
						: `On, sending to ${address}.`
	);
</script>

<section aria-labelledby="{id}-heading" class="card flex flex-col gap-4 p-5 sm:p-6">
	<div class="flex items-start gap-3.5">
		<span class="grid size-9 shrink-0 place-items-center rounded-md bg-well text-fg-muted"><Mail class="size-4" aria-hidden="true" /></span>
		<div class="flex min-w-0 flex-col gap-0.5">
			<h2 id="{id}-heading" class="m-0 text-emphasis font-semibold">Email</h2>
			<p class="m-0 text-small break-words text-fg-muted">{status}</p>
		</div>
	</div>
	<FormMessage {form} />
	{#if returned === 'limited' && !form}<p class="notice m-0">We can't send a confirmation link to that address right now. Try again tomorrow.</p>{/if}
	{#if !available}
		<!-- The status line says so; the inbox itself still gets everything, as the page says. -->
	{:else if !channel}
		{#if returned === 'declined'}<p class="notice m-0">You didn't allow sharing your email address, so email is still off.</p>{/if}
		<form method="POST" action="?/emailOn" class="flex flex-col items-start gap-2">
			<button type="submit" class="btn-primary">Turn on email</button>
			<p class="m-0 text-caption text-fg-subtle">Your account's host asks you before sharing your address with us.</p>
		</form>
	{:else if paused}
		<p class="notice m-0">{paused}</p>
		<div class="flex flex-wrap gap-3">
			<form method="POST" action="?/emailOn"><button type="submit" class="btn-secondary btn-sm">{channel.pausedReason === 'email_unverified' ? 'Send a new link' : 'Check again'}</button></form>
			<form method="POST" action="?/emailOff" use:enhance><button type="submit" class="btn-ghost btn-sm">Turn off email</button></form>
		</div>
	{:else}
		<ChannelTopics action="?/emailTopics" legend="What to email" {topics} selected={channel.topics} />
		<div class="flex flex-wrap gap-3 border-t border-line pt-4">
			<form method="POST" action="?/emailTest" use:enhance><button type="submit" class="btn-secondary btn-sm">Send a test</button></form>
			<form method="POST" action="?/emailOff" use:enhance><button type="submit" class="btn-ghost btn-sm">Turn off email</button></form>
		</div>
	{/if}
</section>
