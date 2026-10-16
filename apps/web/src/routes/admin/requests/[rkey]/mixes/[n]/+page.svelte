<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>Proposed tracklist · {data.mix.title} · Local Network</title>
</svelte:head>

<div class="flex min-w-0 flex-col gap-6">
	<a href="/admin/requests/{data.rkey}" class="inline-flex w-fit items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Back to the request</a>
	<p class="notice notice-info m-0">This tracklist for “{data.mix.title}” is proposed. It’s published as shown here if the request is approved.</p>
	<MixReport report={data.report} title={data.mix.title} listen={data.mix.url} />
</div>
