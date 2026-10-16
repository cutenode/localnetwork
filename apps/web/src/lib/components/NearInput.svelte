<script lang="ts">
	import { LocateFixed, MapPin } from '@lucide/svelte';
	import { searchPlaces, type Place } from '#lib/places.ts';

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

	let results = $state<Place[]>([]);

	let locating = $state(false);

	let timer: ReturnType<typeof setTimeout> | undefined;

	function search() {
		latValue = '';
		lngValue = '';
		clearTimeout(timer);

		if (!geocoder || label.trim().length < 2) {
			results = [];

			return;
		}

		timer = setTimeout(async () => {
			results = await searchPlaces(label);
		}, 250);
	}

	function choose(item: Place) {
		label = item.label;
		latValue = (item.latE6 / 1e6).toFixed(5);
		lngValue = (item.lngE6 / 1e6).toFixed(5);
		results = [];
	}

	function locate() {
		if (!navigator.geolocation) return;
		locating = true;
		navigator.geolocation.getCurrentPosition(
			(position) => {
				latValue = position.coords.latitude.toFixed(5);
				lngValue = position.coords.longitude.toFixed(5);
				label = 'My location';
				locating = false;
			},
			() => (locating = false),
			{ timeout: 10_000, maximumAge: 600_000 }
		);
	}
</script>

<div class="relative">
	<div class="relative">
		<MapPin class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-subtle" />
		<input {id} name="place" class="input pr-10 pl-9" bind:value={label} oninput={search} placeholder="City or venue area" autocomplete="off" />
		<button type="button" class="absolute top-1/2 right-1.5 grid size-8 -translate-y-1/2 place-items-center rounded-md text-fg-muted hover:bg-raised hover:text-fg disabled:opacity-50" aria-label="Use my location" title="Use my location" onclick={locate} disabled={locating}>
			<LocateFixed class="size-4 {locating ? 'animate-pulse' : ''}" />
		</button>
	</div>
	<input type="hidden" name="lat" value={latValue} />
	<input type="hidden" name="lng" value={lngValue} />
	{#if results.length}
		<ul class="absolute z-30 mt-1 max-h-64 w-full overflow-auto rounded-lg border border-line-strong bg-raised p-1 text-small">
			{#each results as item (item.label + item.latE6)}
				<li><button type="button" class="w-full rounded-md px-3 py-2 text-left text-fg-soft hover:bg-well hover:text-fg" onclick={() => choose(item)}>{item.label}</button></li>
			{/each}
		</ul>
	{/if}
</div>
