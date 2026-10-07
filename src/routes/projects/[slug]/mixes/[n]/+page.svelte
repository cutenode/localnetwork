<script lang="ts">
	import { ArrowLeft, ExternalLink } from '@lucide/svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>{data.mix.title} · {data.project.name} · Local Network</title>
	<meta name="description" content="Tracklist of {data.mix.title} by {data.project.name}: what was played, when, and how it was mixed." />
</svelte:head>

<div class="container-page mt-6 space-y-4">
	<a class="menu-link inline-flex items-center gap-1 px-1" href="/projects/{data.project.slug}"><ArrowLeft class="size-4" />{data.project.name}</a>
	<MixReport report={data.report}>
		{#snippet actions()}
			<a class="btn-accent" href={data.mix.url} rel="noopener noreferrer nofollow" target="_blank"><ExternalLink class="size-4" />Listen</a>
		{/snippet}
	</MixReport>
</div>
