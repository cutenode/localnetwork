<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import type { CityFormRow, ProjectFormValues } from '#lib/types.ts';
	import FormSection from './FormSection.svelte';
	import RemoveRow from './RemoveRow.svelte';

	/** Where a promoter books (`section.defs#promoter`): up to 12 cities. */
	let { values = {} }: { values?: ProjectFormValues } = $props();

	// Rows have stable keys, so removing one leaves the others' inputs with their own rows.
	let nextKey = 0;

	const emptyCity = (): CityFormRow & { key: number } => ({ city: '', region: '', country: '', key: nextKey++ });

	const initialCities = () => {
		const list = values.cities ?? [];

		return list.length ? list.map((city) => ({ ...city, key: nextKey++ })) : [emptyCity()];
	};

	let cities = $state<(CityFormRow & { key: number })[]>(initialCities());
</script>

<FormSection title="Where you book" description="The cities you put on shows in, so DJs there can find you.">
	<div class="flex flex-col gap-2">
		{#each cities as row (row.key)}
			<div class="grid grid-cols-[minmax(0,1fr)_5.5rem_2.75rem] gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_7rem_2.75rem]">
				<input class="input" name="bookCity" bind:value={row.city} maxlength="100" placeholder="City" aria-label="City" />
				<input class="input max-sm:hidden" name="bookRegion" bind:value={row.region} maxlength="100" placeholder="Region (optional)" aria-label="Region" />
				<input class="input uppercase" name="bookCountry" bind:value={row.country} minlength="2" maxlength="2" placeholder="DE" aria-label="Country" />
				<RemoveRow label="Remove city" onclick={() => (cities = cities.filter((city) => city.key !== row.key))} />
			</div>
		{/each}
		{#if cities.length < 12}
			<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (cities = [...cities, emptyCity()])}><Plus class="size-4" />Add city</button>
		{/if}
	</div>
</FormSection>
