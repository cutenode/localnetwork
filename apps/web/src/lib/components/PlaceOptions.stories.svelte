<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Combobox } from '#lib/combobox.svelte.ts';
	import type { Place } from '#lib/places.ts';
	import PlaceOptions from './PlaceOptions.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Place options', component: PlaceOptions, tags: ['autodocs'] });

	const places: Place[] = [
		{ label: 'Berlin, Germany', city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
		{ label: 'Berlin, New Hampshire, United States', city: 'Berlin', region: 'New Hampshire', country: 'US', latE6: 44_468_700, lngE6: -71_185_000 },
		{ label: 'Berlingen, Thurgau, Switzerland', city: 'Berlingen', region: 'Thurgau', country: 'CH', latE6: 47_671_000, lngE6: 8_960_000 }
	];

	/** A search that already found `items`, with the arrow keys on `active`. Picking one closes the list. */
	const found = (items: Place[], active = -1) => {
		const combobox = new Combobox<Place>(
			async () => items,
			() => {}
		);

		combobox.items = items;
		combobox.open = items.length > 0;
		combobox.active = active;

		return combobox;
	};

	// One search per story, so picking a place in one doesn't close another's list.
	const suggestions = found(places, 1);

	const onePlace = found(places.slice(0, 1), 0);

	const nothing = found([]);

	const onPhone = found(places, 1);

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<!-- The list under an input that points at it, as PlacePicker and NearInput draw it. -->
{#snippet search(combobox: Combobox<Place>)}
	<div class="relative max-w-md pb-56">
		<label class="label" for="place">Find a city</label>
		<div class="relative">
			<input
				id="place"
				class="input"
				role="combobox"
				aria-autocomplete="list"
				aria-expanded={combobox.open}
				aria-controls="place-options"
				aria-activedescendant={combobox.open && combobox.active >= 0 ? `place-options-${combobox.active}` : undefined}
				value="Berl"
				autocomplete="off"
			/>
			<PlaceOptions {combobox} id="place-options" />
		</div>
	</div>
{/snippet}

<Story name="Suggestions" asChild>{@render search(suggestions)}</Story>

<Story name="One place" asChild>{@render search(onePlace)}</Story>

<Story name="Nothing found" asChild>{@render search(nothing)}</Story>

<Story name="Phone" globals={phone} asChild>{@render search(onPhone)}</Story>
