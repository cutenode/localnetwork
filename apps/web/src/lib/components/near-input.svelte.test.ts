import { page, userEvent } from 'vitest/browser';
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
		await page.getByRole('option', { name: 'Leipzig, Germany' }).click();

		expect(asked).toEqual(['/api/places?q=Leip']);
		await expect.element(near()).toHaveValue('Leipzig, Germany');
		await expect.element(page.getByRole('option', { name: 'Leipzig, Germany' })).not.toBeInTheDocument();
		expect(posted()).toEqual(['51.33970', '12.37310']);
	});

	it('drops the coordinates and the results once the place is typed over', async () => {
		serve({ Le: [leipzig] });
		await render(NearInput, { place: 'Berlin', lat: '52.52000', lng: '13.40500' });
		expect(posted()).toEqual(['52.52000', '13.40500']);

		await near().fill('Le');
		await expect.element(page.getByRole('option', { name: 'Leipzig, Germany' })).toBeVisible();
		await near().fill('L');

		await expect.element(page.getByRole('option', { name: 'Leipzig, Germany' })).not.toBeInTheDocument();
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

	it('picks a place from the keyboard, and closes the list on Escape', async () => {
		serve({ Leip: [leipzig, { ...leipzig, label: 'Leipzig, Ohio', latE6: 41_100_000, lngE6: -84_000_000 }] });
		await render(NearInput);

		await near().fill('Leip');
		await expect.element(near()).toHaveAttribute('aria-expanded', 'true');
		await userEvent.keyboard('{ArrowDown}{ArrowDown}');
		await expect.element(page.getByRole('option', { name: 'Leipzig, Ohio' })).toHaveAttribute('aria-selected', 'true');
		await expect.element(near()).toHaveAttribute('aria-activedescendant', 'near-places-1');
		await userEvent.keyboard('{Enter}');

		await expect.element(near()).toHaveValue('Leipzig, Ohio');
		expect(posted()).toEqual(['41.10000', '-84.00000']);

		await near().fill('Leip');
		await expect.element(page.getByRole('listbox')).toBeVisible();
		await userEvent.keyboard('{Escape}');
		await expect.element(page.getByRole('listbox')).not.toBeInTheDocument();
		await expect.element(near()).toHaveAttribute('aria-expanded', 'false');
	});

	it('says when location is blocked, and leaves the place as it was', async () => {
		vi.spyOn(navigator.geolocation, 'getCurrentPosition').mockImplementation((_, failure) =>
			// SAFETY: NearInput reads only the error's code.
			failure?.({
				code: GeolocationPositionError.PERMISSION_DENIED,
				message: 'User denied Geolocation'
			} as GeolocationPositionError)
		);
		await render(NearInput, { place: 'Berlin', lat: '52.52000', lng: '13.40500' });

		await locate().click();

		await expect.element(locate()).toBeEnabled();
		await expect.element(near()).toHaveValue('Berlin');
		expect(posted()).toEqual(['52.52000', '13.40500']);
		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent('Location is blocked for this site. Type a place instead.');
		await expect
			.element(near())
			.toHaveAccessibleDescription('Location is blocked for this site. Type a place instead.');

		await near().fill('Berl');
		await expect.element(page.getByRole('alert')).not.toBeInTheDocument();
	});

	it("says when the location can't be found", async () => {
		vi.spyOn(navigator.geolocation, 'getCurrentPosition').mockImplementation((_, failure) =>
			// SAFETY: NearInput reads only the error's code.
			failure?.({ code: GeolocationPositionError.TIMEOUT, message: 'Timeout' } as GeolocationPositionError)
		);
		await render(NearInput);

		await locate().click();

		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent("Couldn't find your location. Type a place instead.");
	});

	it('says so in a browser without geolocation', async () => {
		// Shadows the prototype's getter until afterEach deletes it.
		Object.defineProperty(navigator, 'geolocation', { value: undefined, configurable: true });
		await render(NearInput, { id: 'hero-near' });

		await locate().click();

		await expect.element(locate()).toBeEnabled();
		await expect.element(near()).toHaveAttribute('id', 'hero-near');
		await expect.element(near()).toHaveValue('');
		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent("This browser can't share its location. Type a place instead.");
	});
});
