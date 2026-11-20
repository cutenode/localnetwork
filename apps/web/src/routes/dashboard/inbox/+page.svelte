<script lang="ts">
	import { enhance } from '$app/forms';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import InboxList from '#lib/components/InboxList.svelte';
	import type { NotificationChannel, PausedReason } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const SETTINGS = '/dashboard/account/notifications';

	const PAUSED: Record<PausedReason, string> = {
		email_unverified: 'Email starts once you open the link we sent.',
		email_unconfirmed: "Email is paused until your account's host confirms your address.",
		address_rejected: "Email to your address didn't go through, so it's paused.",
		dm_not_allowed: "Bluesky DMs are paused until your chat settings let ours through."
	};

	const SENT: Record<NotificationChannel, string> = { email: 'to your email', dm: 'as Bluesky DMs' };

	const paused = $derived(data.paused.length > 1 ? 'Email and Bluesky DMs are paused.' : data.paused.map((reason) => PAUSED[reason])[0]);

	const alsoSent = $derived(data.alsoSent.length ? `Also sent ${new Intl.ListFormat('en').format(data.alsoSent.map((channel) => SENT[channel]))}.` : null);
</script>

<svelte:head><title>Inbox · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<header class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
		<h1 class="m-0 text-heading font-semibold">Inbox</h1>
		{#if data.unread > 0}
			<form method="POST" action="?/readAll" use:enhance>
				<button type="submit" class="btn-secondary btn-sm">Mark all read</button>
			</form>
		{/if}
	</header>

	<div class="flex flex-col gap-3">
		{#if paused}
			<p class="m-0 flex items-baseline gap-2 text-small text-fg-muted">
				<span class="size-2 shrink-0 translate-y-[-0.0625rem] rounded-full bg-warning" aria-hidden="true"></span>
				<span>{paused} <a class="link" href={SETTINGS}>Notification settings</a></span>
			</p>
		{/if}

		{#await data.inbox}
			<div role="status" aria-label="Loading your inbox"><InboxList loading={4} /></div>
		{:then { rows, older }}
			{#if rows.length}
				<InboxList {rows} />
				{#if older}<Button variant="secondary" size="sm" href="?before={older}" class="self-start">Older</Button>{/if}
			{:else}
				<EmptyState title="Nothing new" body="Booking inquiries, messages and status changes show up here." />
			{/if}
		{:catch}
			<p class="notice notice-info m-0 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">Your inbox didn't load.<a class="btn-secondary btn-sm" href="/dashboard/inbox" data-sveltekit-reload>Try again</a></p>
		{/await}

		<p class="m-0 text-small text-fg-muted">
			{#if alsoSent}{alsoSent} <a class="link" href={SETTINGS}>Change where these go</a>{:else}<a class="link" href={SETTINGS}>Get these by email or Bluesky DM</a>{/if}
		</p>
	</div>
</div>
