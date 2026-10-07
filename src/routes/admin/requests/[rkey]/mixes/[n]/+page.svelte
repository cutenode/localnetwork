<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>Proposed tracklist · {data.mix.title} · Local Network</title>
</svelte:head>

<div class="space-y-4">
	<a class="menu-link inline-flex items-center gap-1 px-1" href="/admin/requests/{data.rkey}"><ArrowLeft class="size-4" />Back to the request</a>
	<p class="notice notice-info">Proposed tracklist for “{data.mix.title}”. It is published as shown here if the request is approved.</p>
	<MixReport report={data.report} />
</div>
