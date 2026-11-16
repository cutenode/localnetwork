<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { Place, PlacesResponse } from '#lib/places.ts';
	import type { AddVenueResponse, VenueOption, VenuesResponse } from '#lib/venues.ts';
	import VenuePicker from './VenuePicker.svelte';

	const venues: VenueOption[] = [
		{ space: 'at://did:plc:root/computer.localnetwork.space.venue/3a', skey: '3a', name: 'Tresor', locality: 'Berlin', country: 'DE' },
		{ space: 'at://did:plc:root/computer.localnetwork.space.venue/3c', skey: '3c', name: 'Tresor', locality: 'Hamburg', country: 'DE' },
		{ space: 'at://did:plc:root/computer.localnetwork.space.venue/3b', skey: '3b', name: 'Tresor.West', locality: 'Dortmund', country: 'DE' }
	];

	const places: Place[] = [{ label: 'Berlin, Germany', city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }];

	/** The venue picker: pick a venue by name, or add one with its city from the city search. */
	const { Story } = defineMeta({
		title: 'Patterns/Venue picker',
		component: VenuePicker,
		// Answers /api/venues and /api/places from the lists above, so the stories never leave the page. Only "Tres" finds venues.
		beforeEach: () => {
			const real = window.fetch;
			window.fetch = async (input, init) => {
				const url = new URL(String(input), location.href);

				if (url.pathname === '/api/venues' && init?.method === 'POST')
					return Response.json({ venue: { ...venues[0], name: 'Kater Blau' } } satisfies AddVenueResponse);

				if (url.pathname === '/api/venues')
					return Response.json({ venues: url.searchParams.get('q') === 'Tres' ? venues : [] } satisfies VenuesResponse);

				if (url.pathname === '/api/places') return Response.json({ places } satisfies PlacesResponse);

				return real(input, init);
			};

			return () => {
				window.fetch = real;
			};
		}
	});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Types `text` into the field named `label`, then waits out the search's pause. */
	const typeInto = async (canvasElement: HTMLElement, label: string, text: string) => {
		const input = [...canvasElement.querySelectorAll<HTMLInputElement>('input')].find(
			(field) => field.labels?.[0]?.textContent?.trim() === label
		);

		if (!input) return;
		input.focus();
		input.value = text;
		input.dispatchEvent(new Event('input', { bubbles: true }));
		await new Promise((resolve) => setTimeout(resolve, 400));
	};

	const results = async ({ canvasElement }: { canvasElement: HTMLElement }) => typeInto(canvasElement, 'Venue', 'Tres');

	const none = async ({ canvasElement }: { canvasElement: HTMLElement }) => typeInto(canvasElement, 'Venue', 'Kater Blau');

	const addForm = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await none({ canvasElement });
		canvasElement.querySelector<HTMLButtonElement>('button.link')?.click();
		await new Promise((resolve) => setTimeout(resolve, 50));
		await typeInto(canvasElement, 'City', 'Berl');
	};
</script>

<Story name="Empty" asChild>
	<div class="max-w-md"><VenuePicker /></div>
</Story>

<Story name="Results" play={results} asChild>
	<div class="max-w-md"><VenuePicker /></div>
</Story>

<Story name="No match" play={none} asChild>
	<div class="max-w-md"><VenuePicker /></div>
</Story>

<Story name="Add form" play={addForm} asChild>
	<div class="max-w-md"><VenuePicker /></div>
</Story>

<Story name="Picked" asChild>
	<div class="max-w-md"><VenuePicker value={venues[0]} /></div>
</Story>

<Story name="Results on a phone" play={results} globals={phone} asChild>
	<div><VenuePicker /></div>
</Story>

<Story name="Add form on a phone" play={addForm} globals={phone} asChild>
	<div><VenuePicker /></div>
</Story>
