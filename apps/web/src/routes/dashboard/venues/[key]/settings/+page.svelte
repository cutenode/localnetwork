<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import VenueSettings from '#lib/components/VenueSettings.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>{data.venue.name} settings · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<SectionHeader level={1} title="{data.venue.name} settings" />
	<FormMessage {form} />
	<VenueSettings approval={data.venue.approval} can={data.can} blocks={data.blocks} values={form && 'values' in form ? form.values : undefined} />
</div>
