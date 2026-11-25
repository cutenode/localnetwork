<script module lang="ts">
	import type { Place } from '#lib/places.ts';
	import type { AddVenueResponse, VenueOption, VenuesResponse } from '#lib/venues.ts';

	/** Venues whose name has `query` in it. An error response, like the rate limit's 429, finds nothing. */
	async function searchVenues(query: string, signal?: AbortSignal): Promise<VenueOption[]> {
		const response = await fetch(`/api/venues?q=${encodeURIComponent(query)}`, { signal });

		if (!response.ok) return [];

		// SAFETY: /api/venues is this app's own endpoint (src/routes/api/venues/+server.ts), and every body it sends `satisfies VenuesResponse`.
		return ((await response.json()) as VenuesResponse).venues;
	}

	/** Adds a venue by its name and the city picked for it, or links to the one already there. */
	async function addVenue(name: string, city: Place): Promise<AddVenueResponse> {
		const response = await fetch('/api/venues', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ name, city })
		});

		// SAFETY: as above; an add answers `satisfies AddVenueResponse`, whatever its status.
		return (await response.json()) as AddVenueResponse;
	}
</script>

<script lang="ts">
	import { MapPin, Plus, Search } from '@lucide/svelte';
	import { tick } from 'svelte';
	import { Combobox } from '#lib/combobox.page-only.svelte.ts';
	import { placeKey, placeText, searchPlaces } from '#lib/places.ts';
	import ComboboxOptions from './ComboboxOptions.svelte';
	import FormMessage from './FormMessage.svelte';

	/**
	 * Picks a venue for a form: an existing one by name, or a new one by its name and its city from the city search.
	 * Names repeat, so each venue shows its city under its name. The form gets the venue's space URI as `name`, and
	 * hears an `input` when the venue changes, as it would from typing, since no field it reads was typed in.
	 */
	let {
		name = 'venue',
		value = $bindable(null),
		required = false,
		id = 'venue'
	}: { name?: string; value?: VenueOption | null; required?: boolean; id?: string } = $props();

	let root = $state<HTMLDivElement>();

	let query = $state('');

	/** The query the last search answered, to tell "nothing matched" from "still asking". */
	let answered = $state('');

	let adding = $state(false);

	let newName = $state('');

	let cityQuery = $state('');

	let city = $state<Place | null>(null);

	let saving = $state(false);

	let error = $state('');

	const venues = new Combobox<VenueOption>(
		async (search, signal) => {
			const found = await searchVenues(search, signal);
			answered = search;

			return found;
		},
		(venue) => void choose(venue),
		250
	);

	const cities = new Combobox<Place>((search) => searchPlaces(search), pickCity, 250);

	const where = (venue: VenueOption) => [venue.locality, venue.country].filter(Boolean).join(', ');

	const venueText = (venue: VenueOption) => venue.name;

	const venueKey = (venue: VenueOption) => venue.space;

	let noMatch = $derived(
		query.trim().length >= 2 && answered === query && !venues.searching && venues.items.length === 0
	);

	function pickCity(place: Place) {
		city = place;
		cityQuery = place.label;
	}

	function startAdding() {
		newName = query.trim();
		error = '';
		adding = true;
	}

	async function choose(venue: VenueOption | null) {
		value = venue;
		await tick();
		root?.dispatchEvent(new Event('input', { bubbles: true }));
	}

	function change() {
		void choose(null);
		query = '';
		answered = '';
	}

	async function add() {
		if (!city) {
			error = 'Pick the city from the search';

			return;
		}

		saving = true;
		error = '';

		try {
			const added = await addVenue(newName, city);

			if ('error' in added) {
				error = added.error;

				return;
			}

			void choose(added.venue);
			adding = false;
		} catch {
			error = "Couldn't add the venue. Check your connection and try again.";
		} finally {
			saving = false;
		}
	}
</script>

<div class="space-y-3" bind:this={root}>
	{#if value}
		<div class="flex items-center justify-between gap-3 rounded-lg border border-line px-3 py-2.5">
			<p class="m-0 flex min-w-0 items-start gap-2">
				<MapPin aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-fg-subtle" />
				<span class="min-w-0">
					<span class="block truncate font-medium text-fg">{value.name}</span>
					<span class="block text-small text-fg-muted">{where(value)}</span>
				</span>
			</p>
			<button type="button" class="btn-ghost btn-sm" onclick={change} aria-label="Change venue">Change</button>
		</div>
		<input type="hidden" {name} value={value.space} />
	{:else if adding}
		<fieldset class="space-y-3 rounded-lg border border-line p-3">
			<legend class="px-1 text-small font-medium text-fg">New venue</legend>
			<div>
				<label class="label" for="{id}-new-name">Name</label>
				<input id="{id}-new-name" class="input" bind:value={newName} maxlength="64" autocomplete="off" />
			</div>
			<div class="relative">
				<label class="label" for="{id}-city">City</label>
				<div class="relative">
					<Search class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-subtle" />
					<input
						id="{id}-city"
						class="input pl-9"
						role="combobox"
						aria-autocomplete="list"
						aria-expanded={cities.open}
						aria-controls="{id}-cities"
						aria-activedescendant={cities.open && cities.active >= 0 ? `${id}-cities-${cities.active}` : undefined}
						bind:value={cityQuery}
						oninput={() => {
							city = null;
							cities.search(cityQuery);
						}}
						onkeydown={cities.onkeydown}
						onblur={cities.close}
						placeholder="Start typing a city…"
						autocomplete="off"
					/>
					<ComboboxOptions combobox={cities} id="{id}-cities" label="Places" noun="place" text={placeText} key={placeKey} />
				</div>
				{#if cities.searching && !cities.open}<p class="hint">Searching…</p>{/if}
			</div>
			<FormMessage form={{ error }} />
			<div class="flex flex-wrap gap-2">
				<button type="button" class="btn-primary btn-sm" onclick={add} disabled={saving || !newName.trim()}>
					{saving ? 'Adding…' : 'Add venue'}
				</button>
				<button type="button" class="btn-ghost btn-sm" onclick={() => (adding = false)} disabled={saving}>Cancel</button>
			</div>
		</fieldset>
	{:else}
		<div class="relative">
			<label class="label" for={id}>Venue</label>
			<div class="relative">
				<Search class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-subtle" />
				<input
					{id}
					class="input pl-9"
					role="combobox"
					aria-autocomplete="list"
					aria-expanded={venues.open}
					aria-controls="{id}-venues"
					aria-activedescendant={venues.open && venues.active >= 0 ? `${id}-venues-${venues.active}` : undefined}
					bind:value={query}
					oninput={() => venues.search(query)}
					onkeydown={venues.onkeydown}
					onblur={venues.close}
					placeholder="Search venues by name…"
					autocomplete="off"
					{required}
				/>
				<ComboboxOptions combobox={venues} id="{id}-venues" label="Venues" noun="venue" text={venueText} detail={where} key={venueKey} />
			</div>
			{#if venues.searching && !venues.open}<p class="hint">Searching…</p>{/if}
		</div>
		{#if noMatch}
			<p class="hint m-0">No venues match "{query.trim()}".</p>
		{/if}
		<button type="button" class="link inline-flex items-center gap-1" onclick={startAdding}>
			<Plus aria-hidden="true" class="size-4" />
			{noMatch ? `Add "${query.trim()}" as a new venue` : 'Add a new venue'}
		</button>
	{/if}
</div>
