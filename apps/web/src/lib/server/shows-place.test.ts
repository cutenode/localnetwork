import { describe, expect, it } from 'vitest';
import { showsPlace } from './shows-place';

/** Cookies kept in a map, the way a browser keeps them between requests. */
function jar() {
	const values = new Map<string, string>();

	return {
		get: (name: string) => values.get(name),
		set: (name: string, value: string) => void values.set(name, value),
		delete: (name: string) => void values.delete(name)
	};
}

const at = (search: string) => new URL(`http://app.test/shows${search}`);

describe('showsPlace', () => {
	it('remembers a picked city and radius, and opens on them next time', () => {
		const cookies = jar();
		const berlin = { place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 25 };

		expect(showsPlace(at('?place=Berlin&lat=52.52&lng=13.405&radius=25'), cookies, true)).toEqual(berlin);
		expect(showsPlace(at(''), cookies, true)).toEqual(berlin);
		expect(showsPlace(at('?view=following'), cookies, true)).toEqual(berlin);
	});

	it("keeps the remembered radius when a form doesn't send one, and falls back to 50 km", () => {
		const cookies = jar();
		showsPlace(at('?place=Berlin&lat=52.52&lng=13.405&radius=10'), cookies, true);

		expect(showsPlace(at('?view=following&place=Hamburg&lat=53.55&lng=9.99'), cookies, true)).toMatchObject({
			place: 'Hamburg',
			radiusKm: 10
		});
		expect(showsPlace(at('?place=Hamburg&lat=53.55&lng=9.99&radius=7'), jar(), true)).toMatchObject({ radiusKm: 50 });
	});

	it("forgets a cleared city, and ignores a cookie it can't read", () => {
		const cookies = jar();
		showsPlace(at('?place=Berlin&lat=52.52&lng=13.405'), cookies, true);

		expect(showsPlace(at('?place='), cookies, true)).toBeNull();
		expect(showsPlace(at(''), cookies, true)).toBeNull();
		cookies.set('ln_shows_place', '{not json');
		expect(showsPlace(at(''), cookies, true)).toBeNull();
	});
});
