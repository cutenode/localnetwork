import { page } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { Place } from '#lib/places.ts';
import { servePlaces as serve } from '#lib/testing/places.ts';
import PlacePicker from './PlacePicker.svelte';

const leipzig: Place = {
	label: 'Leipzig, Saxony, Germany',
	city: 'Leipzig',
	region: 'Saxony',
	country: 'DE',
	latE6: 51_339_700,
	lngE6: 12_373_100
};

const lisbon: Place = {
	label: 'Lisbon, Portugal',
	city: 'Lisbon',
	country: 'PT',
	latE6: 38_722_300,
	lngE6: -9_139_300
};

const search = () => page.getByRole('textbox', { name: 'Find a city' });

const posted = (name: string) => document.querySelector<HTMLInputElement>(`input[name=${name}]`)?.value;

afterEach(() => vi.unstubAllGlobals());

describe('PlacePicker', () => {
	it('starts empty, asking for a city from the search', async () => {
		await render(PlacePicker);

		await expect.element(page.getByLabelText('City')).toHaveValue('');
		await expect.element(page.getByText('Pick a city from the search to enable distance search.')).toBeVisible();
	});

	it('fills the place and its coordinates from a search result', async () => {
		const asked = serve({ Leip: [leipzig] });
		await render(PlacePicker);

		await search().fill('Leip');
		await page.getByRole('button', { name: 'Leipzig, Saxony, Germany' }).click();

		expect(asked).toEqual(['/api/places?q=Leip']);
		await expect.element(page.getByRole('listbox')).not.toBeInTheDocument();
		await expect.element(search()).toHaveValue('Leipzig, Saxony, Germany');
		expect([posted('city'), posted('region'), posted('country'), posted('latE6'), posted('lngE6')]).toEqual([
			'Leipzig',
			'Saxony',
			'DE',
			'51339700',
			'12373100'
		]);
		await expect.element(page.getByText('Coordinates set (51.340, 12.373), so distance search works.')).toBeVisible();
	});

	it('clears the region a place without one leaves behind', async () => {
		serve({ Lis: [lisbon] });
		await render(PlacePicker, { values: { city: 'Leipzig', region: 'Saxony', country: 'DE' } });

		await search().fill('Lis');
		await page.getByRole('button', { name: 'Lisbon, Portugal' }).click();

		await expect.element(page.getByLabelText('Region / state')).toHaveValue('');
		expect([posted('city'), posted('country')]).toEqual(['Lisbon', 'PT']);
	});

	it('says it is searching until the answer comes', async () => {
		let answer: (response: Response) => void = () => {};

		vi.stubGlobal('fetch', () => new Promise<Response>((resolve) => (answer = resolve)));
		await render(PlacePicker);

		await search().fill('Leip');
		await expect.element(page.getByText('Searching…')).toBeVisible();

		answer(Response.json({ places: [leipzig] }));
		await expect.element(page.getByRole('button', { name: 'Leipzig, Saxony, Germany' })).toBeVisible();
		await expect.element(page.getByText('Searching…')).not.toBeInTheDocument();
	});

	it('drops the results once the search is too short to ask', async () => {
		const asked = serve({ Le: [leipzig] });
		await render(PlacePicker);

		await search().fill('Le');
		await expect.element(page.getByRole('listbox')).toBeVisible();
		await search().fill('L');

		await expect.element(page.getByRole('listbox')).not.toBeInTheDocument();
		expect(asked).toEqual(['/api/places?q=Le']);
	});

	it('takes the region and country typed by hand', async () => {
		await render(PlacePicker, { geocoder: false, missing: 'Pick the city from the search.' });

		await page.getByLabelText('Region / state').fill('Saxony');
		await page.getByLabelText('Country').fill('DE');

		expect([posted('region'), posted('country')]).toEqual(['Saxony', 'DE']);
		await expect.element(page.getByText('Pick the city from the search.')).toBeVisible();
		await expect.element(search()).not.toBeInTheDocument();
	});
});
