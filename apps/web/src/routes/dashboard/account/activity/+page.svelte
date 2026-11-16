<script lang="ts">
	import ActivityList from '#lib/components/ActivityList.svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Activity · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-6 sm:gap-8">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">Activity</h1>
		<p class="m-0 text-body text-fg-muted">What you've done on Local Network, newest first.</p>
	</header>
	{#await data.activity}
		<div role="status" aria-label="Loading your activity"><ActivityList loading /></div>
	{:then { entries, older }}
		{#if entries.length}
			<ActivityList {entries} />
			{#if older}<Button variant="secondary" href="?before={older}" class="self-start">Older</Button>{/if}
		{:else}
			<EmptyState title="Nothing yet" body="Bookings, requests and changes you make show up here." />
		{/if}
	{:catch}
		<EmptyState title="Couldn't load your activity" body="Something went wrong on our side. Try again in a minute.">
			<Button variant="secondary" size="sm" href="/dashboard/account/activity" data-sveltekit-reload>Try again</Button>
		</EmptyState>
	{/await}
</div>
