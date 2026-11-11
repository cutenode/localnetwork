<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { Place, PlacesResponse } from '#lib/places.ts';
	import NearInput from './NearInput.svelte';
	import PlacePicker from './PlacePicker.svelte';

	const places: Place[] = [
		{ label: 'Berlin, Germany', city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
		{ label: 'Berlin, New Hampshire, United States', city: 'Berlin', region: 'New Hampshire', country: 'US', latE6: 44_468_700, lngE6: -71_185_000 },
		{ label: 'Berlingen, Thurgau, Switzerland', city: 'Berlingen', region: 'Thurgau', country: 'CH', latE6: 47_671_000, lngE6: 8_960_000 }
	];

	/** The place search on its own: the directory's Near field and the profile forms' city picker, as a combobox. */
	const { Story } = defineMeta({
		title: 'Patterns/Place search',
		// Answers /api/places from the list above, so the stories never leave the page.
		beforeEach: () => {
			const real = window.fetch;
			window.fetch = async (input, init) =>
				String(input).startsWith('/api/places') ? Response.json({ places } satisfies PlacesResponse) : real(input, init);

			return () => {
				window.fetch = real;
			};
		}
	});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Types "Berl" into the first combobox and moves to the second place, the way someone picking with the keyboard would. */
	const typeAndPick = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const input = canvasElement.querySelector<HTMLInputElement>('input[role="combobox"]');

		if (!input) return;
		input.focus();
		input.value = 'Berl';
		input.dispatchEvent(new Event('input', { bubbles: true }));
		await new Promise((resolve) => setTimeout(resolve, 400));
		input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
		input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
	};
</script>

<Story name="Near, suggestions" play={typeAndPick} asChild>
	<div class="max-w-md"><label class="label" for="near">Near</label><NearInput /></div>
</Story>

<Story name="Near, suggestions on a phone" play={typeAndPick} globals={phone} asChild>
	<div><label class="label" for="near">Near</label><NearInput /></div>
</Story>

<Story name="City picker, suggestions" play={typeAndPick} asChild>
	<div class="max-w-2xl"><PlacePicker /></div>
</Story>
