<script lang="ts">
	import { enhance } from '$app/forms';
	import InboxList from '#lib/components/InboxList.svelte';
	import type { PausedReason } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const PAUSED: Record<PausedReason, string> = {
		email_unverified: 'Open the link we emailed you to start getting email.',
		email_unconfirmed: "Email is paused until your account's host confirms your address.",
		address_rejected: "Email to your address didn't go through, so it's paused.",
		dm_not_allowed: "Bluesky chat won't deliver our DMs to you yet."
	};
</script>

<svelte:head><title>Inbox · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<SectionHeader level={1} eyebrow="Inbox" title="What's new" lede="Booking inquiries, messages and status changes in your threads.">
		{#snippet actions()}
			<!-- One row on a phone too: these are small, and stacked they'd read as two unrelated things. -->
			<div class="flex flex-wrap items-center gap-3">
				{#if data.unread > 0}
					<form method="POST" action="?/readAll" use:enhance>
						<button type="submit" class="btn-secondary btn-sm">Mark all read</button>
					</form>
				{/if}
				<Button variant="secondary" size="sm" href="/dashboard/inbox/settings">Settings</Button>
			</div>
		{/snippet}
	</SectionHeader>
	{#each data.paused as reason (reason)}
		<p class="notice m-0">{PAUSED[reason]} <a href="/dashboard/inbox/settings" class="font-medium underline">Settings</a></p>
	{/each}
	<InboxList items={data.items} />
	{#if data.older}
		<Button variant="secondary" href="?before={data.older}" class="self-start">Older</Button>
	{/if}
</div>
