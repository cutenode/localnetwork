<script lang="ts">
	import RequestList from '#lib/components/RequestList.svelte';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Requests · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<SectionHeader level={1} title="Your requests" lede="New projects, claims and changes you've sent to the moderators." />
	<Card padding="none"><RequestList requests={data.requests} /></Card>
</div>
