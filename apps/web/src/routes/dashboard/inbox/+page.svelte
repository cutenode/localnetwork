<script lang="ts">
	import { enhance } from '$app/forms';
	import InboxList from '#lib/components/InboxList.svelte';
	import type { PausedReason } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const PAUSED: Partial<Record<PausedReason, string>> = {
		email_unverified: 'Open the link we emailed you to start getting email.',
		address_rejected: "Email to your address didn't go through, so it's paused."
	};
</script>

<svelte:head><title>Inbox · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<SectionHeader level={1} eyebrow="Inbox" title="What's new" lede="Booking inquiries, messages and status changes in your threads.">
		{#snippet actions()}
			{#if data.unread > 0}
				<form method="POST" action="?/readAll" use:enhance>
					<button type="submit" class="btn-secondary btn-sm">Mark all read</button>
				</form>
			{/if}
			<Button variant="ghost" size="sm" href="/dashboard/inbox/settings">Settings</Button>
		{/snippet}
	</SectionHeader>
	{#if data.emailPaused}
		<p class="notice m-0">{PAUSED[data.emailPaused] ?? "Email is paused until your account's host confirms your address."} <a href="/dashboard/inbox/settings">Email settings</a></p>
	{/if}
	<InboxList items={data.items} />
	{#if data.older}
		<Button variant="secondary" href="?before={data.older}" class="self-start">Older</Button>
	{/if}
</div>
