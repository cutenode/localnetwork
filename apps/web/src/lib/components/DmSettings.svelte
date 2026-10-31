<script module lang="ts">
	import type { DmAvailability, NotificationTopic } from '#lib/types.ts';

	/** What stands in the way of DMs, by what Bluesky chat says now, and what to do about it. */
	const BLOCKED: Partial<Record<DmAvailability, (root: string) => string>> = {
		not_allowed: (root) => `Your Bluesky chat settings don't let us DM you yet. Allow DMs from people you follow, follow @${root}, then check again.`,
		unavailable: () => "We couldn't reach Bluesky chat."
	};
</script>

<script lang="ts">
	import { MessageCircle } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import type { ChannelView } from '#lib/types.ts';
	import ChannelTopics from './ChannelTopics.svelte';
	import FormMessage from './FormMessage.svelte';

	/**
	 * Someone's Bluesky DM setting for their inbox. DMs come from the root account (`root`, its handle), so whether they
	 * can arrive depends on the person's chat settings, which `availability` reports once Bluesky chat answers.
	 */
	let {
		available,
		channel,
		availability,
		root,
		topics,
		form
	}: {
		available: boolean;
		channel: ChannelView | null;
		availability: Promise<DmAvailability> | null;
		root: string;
		topics: NotificationTopic[];
		form?: { error?: string; success?: string } | null;
	} = $props();

	const id = $props.id();

	const status = $derived(
		!available ? 'Not set up on this site.' : !channel ? `Get your inbox as DMs from @${root} too.` : channel.status === 'paused' ? 'Paused.' : `On, from @${root}.`
	);
</script>

{#snippet blocked(state: DmAvailability)}
	{@const reason = BLOCKED[state]?.(root)}
	{#if reason}
		<div class="notice m-0 flex flex-col items-start gap-2.5">
			<p class="m-0">{reason}</p>
			{#if state === 'unavailable'}
				<button type="button" class="btn-secondary btn-sm" onclick={() => invalidateAll()}>Try again</button>
			{:else}
				<a href="https://bsky.app/profile/{root}" class="text-small">@{root} on Bluesky</a>
			{/if}
		</div>
	{/if}
{/snippet}

<section aria-labelledby="{id}-heading" class="card flex flex-col gap-4 p-5 sm:p-6">
	<div class="flex items-start gap-3.5">
		<span class="grid size-9 shrink-0 place-items-center rounded-md bg-well text-fg-muted"><MessageCircle class="size-4" aria-hidden="true" /></span>
		<div class="flex min-w-0 flex-col gap-0.5">
			<h2 id="{id}-heading" class="m-0 text-emphasis font-semibold">Bluesky DMs</h2>
			<p class="m-0 text-small break-words text-fg-muted">{status}</p>
		</div>
	</div>
	<FormMessage {form} />
	{#if available && availability}
		{#await availability}
			<p class="m-0 flex min-h-5 items-center" aria-busy="true"><span class="h-3.5 w-2/3 animate-pulse rounded-sm bg-raised" aria-hidden="true"></span><span class="sr-only">Checking Bluesky chat</span></p>
		{:then state}
			{#if state === 'ok'}
				<p class="m-0 flex min-h-5 items-center text-small text-fg-muted">Bluesky chat can deliver DMs from @{root} to you.</p>
			{:else}
				{@render blocked(state)}
			{/if}
		{/await}
	{/if}
	{#if !available}
		<!-- The status line says so; the inbox itself still gets everything, as the page says. -->
	{:else if !channel}
		<form method="POST" action="?/dmOn" use:enhance><button type="submit" class="btn-primary">Turn on DMs</button></form>
	{:else if channel.status === 'paused'}
		<div class="flex flex-wrap gap-3">
			<form method="POST" action="?/dmOn" use:enhance><button type="submit" class="btn-secondary btn-sm">Check again</button></form>
			<form method="POST" action="?/dmOff" use:enhance><button type="submit" class="btn-ghost btn-sm">Turn off DMs</button></form>
		</div>
	{:else}
		<ChannelTopics action="?/dmTopics" legend="What to DM" {topics} selected={channel.topics} />
		<div class="flex flex-wrap gap-3 border-t border-line pt-4">
			<form method="POST" action="?/dmTest" use:enhance><button type="submit" class="btn-secondary btn-sm">Send a test</button></form>
			<form method="POST" action="?/dmOff" use:enhance><button type="submit" class="btn-ghost btn-sm">Turn off DMs</button></form>
		</div>
	{/if}
</section>
