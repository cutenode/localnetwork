<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import WaitingEvents from '#lib/components/WaitingEvents.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>{data.venue.name} events · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<SectionHeader level={1} title="{data.venue.name} events" lede="Approve an event naming {data.venue.name} to mark it Confirmed." />
	<FormMessage {form} />
	<WaitingEvents waiting={data.waiting} />
</div>
