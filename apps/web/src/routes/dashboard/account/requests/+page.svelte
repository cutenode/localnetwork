<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import RequestList from '#lib/components/RequestList.svelte';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Requests to staff · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-6 sm:gap-8">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">Requests to staff</h1>
		<p class="m-0 text-body text-fg-muted">What you've asked staff for, and where each one stands.</p>
	</header>
	{#await data.requests}
		<div role="status" aria-label="Loading your requests"><RequestList loading /></div>
	{:then requests}
		<RequestList {requests} />
	{:catch}
		<EmptyState title="Couldn't load your requests" body="Something went wrong on our side. Try again in a minute.">
			<Button variant="secondary" size="sm" href="/dashboard/account/requests" data-sveltekit-reload>Try again</Button>
		</EmptyState>
	{/await}
</div>
