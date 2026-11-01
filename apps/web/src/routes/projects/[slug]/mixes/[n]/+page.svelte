<script lang="ts">
	import LinkedSet from '#lib/components/LinkedSet.svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import PreviewBanner from '#lib/components/PreviewBanner.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-6 py-10 sm:py-14">
	<PreviewBanner status={data.project.status} />
	<MixReport report={data.report} title={data.mix.title} listen={data.mix.url} source={data.source}>
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
