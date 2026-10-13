<script lang="ts">
	import { SlidersHorizontal, X } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import Pagination from '#lib/components/Pagination.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import { pluralize } from '#lib/format.ts';
	import { FEE_TIERS, FORMATS, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const f = $derived(data.filters);

	let showFilters = $state(false);

	const anyFilter = $derived(!!(f.q || f.lat || f.genres.length || f.kinds.length || f.formats.length || f.setTypes.length || f.feeTiers.length || f.country || f.unclaimed));
</script>

<svelte:head><title>Find DJs · Local Network</title></svelte:head>

<div class="container-page py-10">
	<div class="mb-8 flex flex-wrap items-end justify-between gap-4">
		<div>
			<p class="eyebrow">Directory</p>
			<h1 class="mt-1 text-4xl">Find DJs</h1>
			<p class="muted mt-1">{pluralize(data.result.total, 'project')}{f.place ? ` within ${f.radiusKm} km of ${f.place}` : ''}</p>
		</div>
		<div class="flex gap-2">
			<a href="/match" class="btn-secondary">Match a specific show</a>
			<button class="btn-secondary lg:hidden" onclick={() => (showFilters = !showFilters)}><SlidersHorizontal class="size-4" />Filters</button>
		</div>
	</div>

	<div class="grid gap-8 lg:grid-cols-[18rem_1fr]">
		<aside class="{showFilters ? 'block' : 'hidden'} lg:block">
			<form method="GET" class="card card-pad sticky top-20 space-y-5" data-sveltekit-reset="false">
				<div>
					<label class="label" for="q">Search</label>
					<input id="q" name="q" class="input" value={f.q} placeholder="Name, sound, venue…" />
				</div>
				<div>
					<label class="label" for="near">Near</label>
					<NearInput place={f.place} lat={f.lat} lng={f.lng} geocoder={data.geocoder} />
					<label class="label mt-3" for="radius">Within</label>
					<select id="radius" name="radius" class="input">
						{#each [10, 25, 50, 100, 250, 500, 2000] as km (km)}<option value={km} selected={f.radiusKm === km}>{km} km</option>{/each}
					</select>
				</div>
				<div>
					<label class="label" for="genres">Genres</label>
					<GenreInput value={f.genres.join(', ')} placeholder="Any genre" />
				</div>
				<fieldset>
					<legend class="label">Fee tier</legend>
					<div class="space-y-1.5">
						{#each FEE_TIERS as option (option.value)}
							<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="fee" value={option.value} checked={f.feeTiers.includes(option.value)} />{option.label}</label>
						{/each}
					</div>
				</fieldset>
				<fieldset>
					<legend class="label">Slot</legend>
					<div class="flex flex-wrap gap-1.5">
						{#each SET_TYPES as option (option.value)}
							<label class="chip-toggle"><input type="checkbox" class="sr-only" name="setType" value={option.value} checked={f.setTypes.includes(option.value)} />{option.label}</label>
						{/each}
					</div>
				</fieldset>
				<details class="group" open={f.kinds.length > 0 || f.formats.length > 0 || !!f.country || f.unclaimed}>
					<summary class="label cursor-pointer">More filters</summary>
					<div class="mt-3 space-y-4">
						<fieldset>
							<legend class="label">Type</legend>
							{#each PROJECT_KINDS as option (option.value)}
								<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="kind" value={option.value} checked={f.kinds.includes(option.value)} />{option.label}</label>
							{/each}
						</fieldset>
						<fieldset>
							<legend class="label">Format</legend>
							{#each FORMATS as option (option.value)}
								<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="format" value={option.value} checked={f.formats.includes(option.value)} />{option.label}</label>
							{/each}
						</fieldset>
						<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="unclaimed" checked={f.unclaimed} />Unclaimed only</label>
						<div>
							<label class="label" for="country">Country code</label>
							<input id="country" name="country" class="input uppercase" maxlength="2" value={f.country} placeholder="Any" />
						</div>
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
				<div class="flex gap-2">
					<button class="btn-primary flex-1">Apply</button>
					{#if anyFilter}<a href="/projects" class="btn-ghost" aria-label="Clear filters"><X class="size-4" /></a>{/if}
				</div>
			</form>
		</aside>

		<section>
			{#if data.result.items.length === 0}
				<EmptyState title="No projects match those filters" body="Try a wider radius, fewer genres, or a different slot. Know a DJ who should be here? Ask them to list their project.">
					<a href="/projects" class="btn-secondary">Clear filters</a>
				</EmptyState>
			{:else}
				<div class="grid gap-5 sm:grid-cols-2">
					{#each data.result.items as project (project.uri)}<ProjectCard {project} />{/each}
				</div>
				<Pagination page={data.result.page} pageSize={data.result.pageSize} total={data.result.total} />
			{/if}
		</section>
	</div>
</div>
