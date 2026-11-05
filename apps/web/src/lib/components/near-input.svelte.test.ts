import { page } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { Place } from '#lib/places.ts';
import { servePlaces as serve } from '#lib/testing/places.ts';
import NearInput from './NearInput.svelte';

const leipzig: Place = {
	label: 'Leipzig, Germany',
	city: 'Leipzig',
	country: 'DE',
	latE6: 51_339_700,
	lngE6: 12_373_100
};

const near = () => page.getByPlaceholder('City or venue area');

const locate = () => page.getByRole('button', { name: 'Use my location' });

const posted = () =>
	['lat', 'lng'].map((name) => document.querySelector<HTMLInputElement>(`input[name=${name}]`)?.value);

afterEach(() => {
	vi.unstubAllGlobals();
	vi.restoreAllMocks();
	Reflect.deleteProperty(navigator, 'geolocation');
});

describe('NearInput', () => {
	it('posts a picked place with its coordinates', async () => {
		const asked = serve({ Leip: [leipzig] });
		await render(NearInput);

		await near().fill('Leip');
		await page.getByRole('button', { name: 'Leipzig, Germany' }).click();

		expect(asked).toEqual(['/api/places?q=Leip']);
		await expect.element(near()).toHaveValue('Leipzig, Germany');
		await expect.element(page.getByRole('button', { name: 'Leipzig, Germany' })).not.toBeInTheDocument();
		expect(posted()).toEqual(['51.33970', '12.37310']);
	});

	it('drops the coordinates and the results once the place is typed over', async () => {
		serve({ Le: [leipzig] });
		await render(NearInput, { place: 'Berlin', lat: '52.52000', lng: '13.40500' });
		expect(posted()).toEqual(['52.52000', '13.40500']);

		await near().fill('Le');
		await expect.element(page.getByRole('button', { name: 'Leipzig, Germany' })).toBeVisible();
		await near().fill('L');

		await expect.element(page.getByRole('button', { name: 'Leipzig, Germany' })).not.toBeInTheDocument();
		expect(posted()).toEqual(['', '']);
	});

	it('searches nothing without a geocoder', async () => {
		const asked = serve({ Leip: [leipzig] });
		await render(NearInput, { geocoder: false, place: 'Berlin', lat: '52.52000', lng: '13.40500' });

		await near().fill('Leip');

		await expect.element(near()).toHaveValue('Leip');
		expect(posted()).toEqual(['', '']);
		expect(asked).toEqual([]);
	});

	it("uses the device's location", async () => {
		let report: PositionCallback = () => {};

		vi.spyOn(navigator.geolocation, 'getCurrentPosition').mockImplementation((success) => (report = success));
		await render(NearInput);

		await locate().click();
		await expect.element(locate()).toBeDisabled();
		// SAFETY: NearInput reads only the position's latitude and longitude.
		report({ coords: { latitude: 51.3397, longitude: 12.3731 } } as GeolocationPosition);

		await expect.element(near()).toHaveValue('My location');
		await expect.element(locate()).toBeEnabled();
		expect(posted()).toEqual(['51.33970', '12.37310']);
	});

	it("leaves the place as it was when the location can't be had", async () => {
		vi.spyOn(navigator.geolocation, 'getCurrentPosition').mockImplementation((_, failure) =>
			// SAFETY: NearInput only learns that the position failed, not how.
			failure?.({ code: 1, message: 'User denied Geolocation' } as GeolocationPositionError)
		);
		await render(NearInput, { place: 'Berlin', lat: '52.52000', lng: '13.40500' });

		await locate().click();

		await expect.element(locate()).toBeEnabled();
		await expect.element(near()).toHaveValue('Berlin');
		expect(posted()).toEqual(['52.52000', '13.40500']);
	});

	it('does nothing in a browser without geolocation', async () => {
		// Shadows the prototype's getter until afterEach deletes it.
		Object.defineProperty(navigator, 'geolocation', { value: undefined, configurable: true });
		await render(NearInput, { id: 'hero-near' });

		await locate().click();

		await expect.element(locate()).toBeEnabled();
		await expect.element(near()).toHaveAttribute('id', 'hero-near');
		await expect.element(near()).toHaveValue('');
	});
});
