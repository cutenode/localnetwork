<script lang="ts">
	import { LocateFixed, MapPin } from '@lucide/svelte';
	import { Combobox } from '#lib/combobox.svelte.ts';
	import { searchPlaces, type Place } from '#lib/places.ts';
	import PlaceOptions from './PlaceOptions.svelte';

	/**
	 * The "near" field of a search: a place picked from suggestions, or the device's location, posted as `place` with
	 * its `lat` and `lng`. Typing again drops the coordinates, since they belonged to the place picked.
	 */
	let {
		place = '',
		lat = '',
		lng = '',
		geocoder = true,
		id = 'near'
	}: { place?: string; lat?: string; lng?: string; geocoder?: boolean; id?: string } = $props();

	let label = $derived(place);

	let latValue = $derived(lat);

	let lngValue = $derived(lng);

	let locating = $state(false);

	/** Why the device's location couldn't be used, until the next try. */
	let locateError = $state('');

	const listId = $derived(`${id}-places`);

	const places = new Combobox<Place>(
		(query) => searchPlaces(query),
		(item) => {
			label = item.label;
			latValue = (item.latE6 / 1e6).toFixed(5);
			lngValue = (item.lngE6 / 1e6).toFixed(5);
		},
		250
	);

	function oninput() {
		latValue = '';
		lngValue = '';
		locateError = '';

		if (geocoder) places.search(label);
	}

	function locate() {
		locateError = '';

		if (!navigator.geolocation) {
			locateError = "This browser can't share its location. Type a place instead.";

			return;
		}

		locating = true;
		navigator.geolocation.getCurrentPosition(
			(position) => {
				latValue = position.coords.latitude.toFixed(5);
				lngValue = position.coords.longitude.toFixed(5);
				label = 'My location';
				locating = false;
			},
			(error) => {
				locating = false;
				locateError = error.code === GeolocationPositionError.PERMISSION_DENIED ? 'Location is blocked for this site. Type a place instead.' : "Couldn't find your location. Type a place instead.";
			},
			{ timeout: 10_000, maximumAge: 600_000 }
		);
	}
</script>

<div class="relative">
	<div class="relative">
		<MapPin aria-hidden="true" class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-subtle" />
		<input
			{id}
			name="place"
			class="input pr-10 pl-9"
			role="combobox"
			aria-autocomplete="list"
			aria-expanded={places.open}
			aria-controls={listId}
			aria-activedescendant={places.open && places.active >= 0 ? `${listId}-${places.active}` : undefined}
			aria-describedby={locateError ? `${id}-locate-error` : undefined}
			bind:value={label}
			{oninput}
			onkeydown={places.onkeydown}
			onblur={places.close}
			placeholder="City or venue area"
			autocomplete="off"
		/>
		<button type="button" class="absolute top-1/2 right-1.5 grid size-8 -translate-y-1/2 place-items-center rounded-md text-fg-muted hover:bg-raised hover:text-fg disabled:opacity-50" aria-label="Use my location" title="Use my location" onclick={locate} disabled={locating}>
			<LocateFixed class="size-4 {locating ? 'animate-pulse' : ''}" />
		</button>
		<PlaceOptions combobox={places} id={listId} class="z-30" />
	</div>
	<input type="hidden" name="lat" value={latValue} />
	<input type="hidden" name="lng" value={lngValue} />
	{#if locateError}<p id="{id}-locate-error" class="hint text-critical" role="alert">{locateError}</p>{/if}
</div>
