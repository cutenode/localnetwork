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
	<SectionHeader level={1} eyebrow="Charts" tone="high" title="What our DJs play" align="split" lede="Tracks ranked by how many DJs played them. Each DJ counts once per track, a track needs three DJs to chart, and an artist gets at most five supports from one DJ.">
		{#snippet actions()}<Button variant="secondary" href="/artists">Look up an artist</Button>{/snippet}
	</SectionHeader>

	<form method="GET" class="card grid gap-4 p-5 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-end" data-sveltekit-reset="false">
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

	<ChartBoard charts={data.charts} retryHref={page.url.pathname + page.url.search} />
</div>
