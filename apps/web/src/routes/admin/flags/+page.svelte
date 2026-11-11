<script lang="ts">
	import FlagTable from '#lib/components/FlagTable.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Flags · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Flags" lede="Turn a feature on for everyone, a share of accounts, groups or single accounts. The website and locallibrary give an account the same answer." />
	<FlagTable flags={data.flags} />
</div>
