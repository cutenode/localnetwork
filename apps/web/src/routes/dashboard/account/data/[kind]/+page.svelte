<script lang="ts">
	import HeldRecordList from '#lib/components/HeldRecordList.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Your data · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-6 sm:gap-8">
	<h1 class="m-0 text-heading font-semibold">Your data</h1>
	<HeldRecordList records={data.records} {form} retry="/dashboard/account/data/{data.kind}" />
</div>
