<script lang="ts">
	import LinkedSet from '#lib/components/LinkedSet.svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import { pluralize } from '#lib/format.ts';
	import Monogram from '#lib/ui/Monogram.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const description = $derived(
		`The tracklist of ${data.mix.title} by ${data.project.name}: ${pluralize(data.report.plays.length, 'track')}, when each plays, and how they were mixed.`
	);
</script>

<svelte:head>
	<title>{data.mix.title} by {data.project.name} · Local Network</title>
	<meta name="description" content={description} />
</svelte:head>

<div class="container-page py-10 sm:py-14">
	<MixReport report={data.report} title={data.mix.title} listen={data.mix.url}>
		{#snippet details()}
			{#if data.mix.set && data.sets}<LinkedSet linked={data.mix.set} sets={data.sets} />{/if}
		{/snippet}
		{#snippet project()}
			<a href="/projects/{data.project.slug}" class="flex w-fit items-center gap-2.5 font-semibold text-fg no-underline hover:underline">
				{#if data.project.avatarUrl}
					<img src={data.project.avatarUrl} alt="" class="size-8 shrink-0 rounded-[9px] object-cover" />
				{:else}
					<Monogram name={data.project.name} size="sm" />
				{/if}
				{data.project.name}
			</a>
		{/snippet}
	</MixReport>
</div>
