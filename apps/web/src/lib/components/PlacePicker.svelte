<script lang="ts">
	import { LocateFixed, Search } from '@lucide/svelte';
	import { searchPlaces, type Place } from '#lib/places.ts';
	import type { ProjectFormValues } from '#lib/types.ts';

	let {
		values = {},
		geocoder = true,
		required = true
	}: { values?: Pick<ProjectFormValues, 'city' | 'region' | 'country' | 'latE6' | 'lngE6'>; geocoder?: boolean; required?: boolean } = $props();

	let city = $derived(values.city ?? '');

	let region = $derived(values.region ?? '');

	let country = $derived(values.country ?? '');

	let latE6 = $derived(String(values.latE6 ?? ''));

	let lngE6 = $derived(String(values.lngE6 ?? ''));

	let query = $state('');

	let results = $state<Place[]>([]);

	let searching = $state(false);

	let timer: ReturnType<typeof setTimeout> | undefined;

	function search() {
		clearTimeout(timer);

		if (query.trim().length < 2) {
			results = [];

			return;
		}

		timer = setTimeout(async () => {
			searching = true;

			try {
				results = await searchPlaces(query);
			} finally {
				searching = false;
			}
		}, 250);
	}

	// Coordinates belong to the picked place, so typing a different one drops them.
	function edited() {
		latE6 = '';
		lngE6 = '';
	}

	function choose(place: Place) {
		city = place.city;
		region = place.region ?? '';
		country = place.country;
		latE6 = String(place.latE6);
		lngE6 = String(place.lngE6);
		query = place.label;
		results = [];
	}
</script>

<div class="space-y-3">
	{#if geocoder}
		<div class="relative">
			<label class="label" for="place-search">Find a city</label>
			<div class="relative">
				<Search class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ansi-haze" />
				<input id="place-search" class="input pl-9" bind:value={query} oninput={search} placeholder="Start typing a city…" autocomplete="off" />
			</div>
			{#if results.length}
				<ul class="card absolute z-20 mt-1 max-h-64 w-full overflow-auto p-1 text-sm" role="listbox">
					{#each results as place (place.label + place.latE6)}
						<li>
							<button type="button" class="w-full px-3 py-1 text-left hover:bg-ansi-blue hover:text-ansi-white" onclick={() => choose(place)}>{place.label}</button>
						</li>
					{/each}
				</ul>
			{:else if searching}
				<p class="hint">Searching…</p>
			{/if}
		</div>
	{/if}
	<div class="grid gap-3 sm:grid-cols-[1fr_1fr_7rem]">
		<div>
			<label class="label" for="city">City</label>
			<input id="city" class="input" name="city" bind:value={city} oninput={edited} {required} maxlength="100" />
		</div>
		<div>
			<label class="label" for="region">Region / state</label>
			<input id="region" class="input" name="region" bind:value={region} oninput={edited} maxlength="100" />
		</div>
		<div>
			<label class="label" for="country">Country</label>
			<input id="country" class="input uppercase" name="country" bind:value={country} oninput={edited} {required} minlength="2" maxlength="2" placeholder="US" />
		</div>
	</div>
	<input type="hidden" name="latE6" value={latE6} />
	<input type="hidden" name="lngE6" value={lngE6} />
	<p class="hint flex items-center gap-1">
		<LocateFixed class="size-3" />
		{#if latE6 && lngE6}Coordinates set ({(Number(latE6) / 1e6).toFixed(3)}, {(Number(lngE6) / 1e6).toFixed(3)}), so distance search works.{:else}Pick a city from the search to enable distance search.{/if}
	</p>
</div>
