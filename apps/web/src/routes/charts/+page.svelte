<script lang="ts">
	import { page } from '$app/state';
	import ChartBoard from '#lib/components/supports/ChartBoard.svelte';
	import { CHART_DAYS, monthLabel, placeValue } from '#lib/supports.ts';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const f = $derived(data.filters);
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Charts" tone="high" title="What our DJs play" align="split" lede="The tracks the most DJs played.">
		{#snippet actions()}<Button variant="secondary" href="/artists">Look up an artist</Button>{/snippet}
	</SectionHeader>

	<details class="text-small text-fg-muted">
		<summary class="cursor-pointer font-medium text-fg-soft">How charts count</summary>
		<p class="mt-2 mb-0 max-w-[65ch]">Each DJ counts once per track, a track needs three DJs to chart, and an artist gets at most five supports from one DJ.</p>
	</details>

	<!-- Folded above the charts on a phone, always open on a wider screen. -->
	<details class="group card sm:details-content:[content-visibility:visible]">
		<summary class="flex min-h-12 cursor-pointer list-none items-center gap-2 px-5 font-semibold sm:hidden [&::-webkit-details-marker]:hidden">
			<span class="min-w-0 truncate">{f.month ? monthLabel(f.month) : `Last ${CHART_DAYS} days`} · {f.place || 'Everywhere'}</span>
			<span class="ml-auto text-small font-normal text-fg-muted group-open:hidden">Change</span>
			<span class="ml-auto hidden text-small font-normal text-fg-muted group-open:inline">Hide</span>
		</summary>
	<form method="GET" class="grid gap-4 border-line p-5 max-sm:border-t sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-end" data-sveltekit-reset="false">
		<div class="min-w-0">
			<label class="label" for="month">When</label>
			<select id="month" name="month" class="input">
				<option value="" selected={!f.month}>Last {CHART_DAYS} days</option>
				{#each data.months as month (month)}<option value={month} selected={f.month === month}>{monthLabel(month)}</option>{/each}
			</select>
		</div>
		<div class="min-w-0">
			<label class="label" for="place">Where</label>
			<select id="place" name="place" class="input">
				<option value="" selected={!f.place}>Everywhere</option>
				<!-- The cities come with the charts; until then the one picked stays picked. -->
				{#snippet picked(places: string[])}
					{#if f.place && !places.includes(f.place)}<option value={f.place} selected>{f.place}</option>{/if}
				{/snippet}
				{#await data.charts}
					{@render picked([])}
				{:then charts}
					{@const places = (charts?.places ?? []).map(placeValue)}
					{@render picked(places)}
					{#each places as place (place)}<option value={place} selected={f.place === place}>{place}</option>{/each}
				{:catch}
					{@render picked([])}
				{/await}
			</select>
		</div>
		<button class="btn-primary">Show charts</button>
	</form>
	</details>

	<ChartBoard charts={data.charts} retryHref={page.url.pathname + page.url.search} />
</div>
