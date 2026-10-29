<script lang="ts">
	import { enhance } from '$app/forms';
	import InboxList from '#lib/components/InboxList.svelte';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
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
		{/snippet}
	</SectionHeader>
	<InboxList items={data.items} />
	{#if data.older}
		<Button variant="secondary" href="?before={data.older}" class="self-start">Older</Button>
	{/if}
</div>
