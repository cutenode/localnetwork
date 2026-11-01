<script lang="ts">
	import SharedSpaceTable from '#lib/components/SharedSpaceTable.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Spaces · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Spaces" lede="Acts, parties and venues, each in a space under the root account. Staff can add and remove anyone, and list them in the directory." />
	<SharedSpaceTable spaces={data.spaces} />
</div>
