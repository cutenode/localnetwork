<script lang="ts">
	import { SlidersHorizontal } from '@lucide/svelte';
	import { page } from '$app/state';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import FindDjsSwitch from '#lib/components/FindDjsSwitch.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import Pagination from '#lib/components/Pagination.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import { pluralize } from '#lib/format.ts';
	import { FEE_TIERS, FORMATS, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const f = $derived(data.filters);

	/** How many filters are on, for the phone's folded panel. Sort isn't a filter. */
	const active = $derived(
		[f.q, f.lat, f.country, f.unclaimed].filter(Boolean).length +
			[f.genres, f.kinds, f.formats, f.setTypes, f.feeTiers].reduce((sum, list) => sum + list.length, 0)
	);
</script>

<svelte:head><title>Find DJs · Local Network</title></svelte:head>

{#snippet chips(name: string, options: readonly { value: string; label: string; hint?: string }[], checked: string[])}
	<div class="flex flex-wrap gap-1.5">
		{#each options as option (option.value)}
			<label class="chip-toggle" title={option.hint}><input type="checkbox" class="sr-only" {name} value={option.value} checked={checked.includes(option.value)} />{option.label}</label>
		{/each}
	</div>
{/snippet}

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<div class="flex flex-col gap-6">
		<FindDjsSwitch current="/projects" />
		<SectionHeader level={1} eyebrow="Directory" title="Find DJs" align="split" lede="Search by sound, place, fee and slot. A search for house finds deep house and afro house too." />
	</div>

	<div class="grid items-start gap-6 lg:grid-cols-[18rem_minmax(0,1fr)] lg:gap-10">
		<!-- Folded above the results on a phone, always open beside them on a wide screen. -->
		<details class="group card lg:sticky lg:top-20 lg:details-content:[content-visibility:visible]">
			<summary class="flex min-h-12 cursor-pointer list-none items-center gap-2 px-5 font-semibold lg:hidden [&::-webkit-details-marker]:hidden">
				<SlidersHorizontal class="size-4 text-fg-muted" />Filters
				{#if active}<span class="pill bg-raised text-fg-soft">{active} on</span>{/if}
				<span class="ml-auto text-small font-normal text-fg-muted group-open:hidden">Show</span>
				<span class="ml-auto hidden text-small font-normal text-fg-muted group-open:inline">Hide</span>
			</summary>
			<form method="GET" class="flex flex-col gap-5 border-line p-5 max-lg:border-t" data-sveltekit-reset="false">
				<div>
					<label class="label" for="q">Search</label>
					<input id="q" name="q" class="input" value={f.q} placeholder="Name, sound, venue" />
				</div>
				<div>
					<label class="label" for="near">Near</label>
					<NearInput place={f.place} lat={f.lat} lng={f.lng} geocoder={data.geocoder} />
				</div>
				<div>
					<label class="label" for="radius">Within</label>
					<select id="radius" name="radius" class="input">
						{#each [10, 25, 50, 100, 250, 500, 2000] as km (km)}<option value={km} selected={f.radiusKm === km}>{km} km</option>{/each}
					</select>
				</div>
				<div>
					<label class="label" for="genres">Genres</label>
					<GenreInput value={f.genres.join(', ')} placeholder="Any genre" />
				</div>
				<fieldset class="m-0 border-0 p-0">
					<legend class="label">Fee tier</legend>
					{@render chips('fee', FEE_TIERS, f.feeTiers)}
				</fieldset>
				<fieldset class="m-0 border-0 p-0">
					<legend class="label">Slot</legend>
					{@render chips('setType', SET_TYPES, f.setTypes)}
				</fieldset>
				<details open={f.kinds.length > 0 || f.formats.length > 0 || !!f.country || f.unclaimed}>
					<summary class="cursor-pointer text-small font-semibold text-fg-soft">More filters</summary>
					<div class="mt-4 flex flex-col gap-5">
						<fieldset class="m-0 border-0 p-0">
							<legend class="label">Format</legend>
							{@render chips('format', FORMATS, f.formats)}
						</fieldset>
						<fieldset class="m-0 border-0 p-0">
							<legend class="label">Kind</legend>
							{@render chips('kind', PROJECT_KINDS, f.kinds)}
						</fieldset>
						<div>
							<label class="label" for="country">Country code</label>
							<input id="country" name="country" class="input uppercase" maxlength="2" value={f.country} placeholder="Any" />
						</div>
						<label class="flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="unclaimed" checked={f.unclaimed} />Unclaimed only</label>
					</div>
				</details>
				<div>
					<label class="label" for="sort">Sort by</label>
					<select id="sort" name="sort" class="input">
						<option value="" selected={!f.sort}>Best match</option>
						<option value="distance" selected={f.sort === 'distance'}>Distance</option>
						<option value="recent" selected={f.sort === 'recent'}>Newest</option>
						<option value="name" selected={f.sort === 'name'}>Name</option>
					</select>
				</div>
				<button class="btn-primary">Apply</button>
			</form>
		</details>

		<section aria-labelledby="results-title" class="flex min-w-0 flex-col gap-5">
			{#if !data.result}
				<h2 id="results-title" class="sr-only">Results</h2>
				<EmptyState title="Directory offline" body="The DJ directory isn't answering. Try again in a minute.">
					<Button variant="secondary" href={page.url.pathname + page.url.search}>Try again</Button>
				</EmptyState>
			{:else}
				<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
					<h2 id="results-title" class="m-0 text-body font-semibold">
						{pluralize(data.result.total, 'DJ')}{#if f.place}<span class="font-normal text-fg-muted"> within {f.radiusKm} km of {f.place}</span>{/if}
					</h2>
					{#if active && data.result.items.length}<a href="/projects" class="text-small text-fg-muted no-underline hover:text-fg">Clear filters</a>{/if}
				</div>
				{#if data.result.items.length === 0}
					<EmptyState title="No DJs match those filters" body="Try a wider radius, fewer genres or another slot.">
						<Button variant="secondary" href="/projects">Clear filters</Button>
					</EmptyState>
				{:else}
					<ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(min(100%,17rem),1fr))] gap-4 p-0">
						{#each data.result.items as project (project.uri)}<li class="min-w-0"><ProjectCard {project} /></li>{/each}
					</ul>
					<Pagination page={data.result.page} pageSize={data.result.pageSize} total={data.result.total} />
				{/if}
			{/if}
		</section>
	</div>
</div>
