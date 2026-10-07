import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type Db, openDatabase } from './db';
import { Geocoder } from './geocode';
import { silentLogger } from './logger';

describe('Geocoder', () => {
	let db: Db;
	beforeEach(async () => {
		db = await openDatabase(':memory:');
	});
	afterEach(() => db.destroy());

	it('searches Open-Meteo, maps results to E6 coordinates, and caches them', async () => {
		const fetch = vi.fn<typeof globalThis.fetch>(async () =>
			Response.json({
				results: [
					{
						name: 'Berlin',
						admin1: 'Land Berlin',
						country_code: 'de',
						country: 'Germany',
						latitude: 52.52437,
						longitude: 13.41053
					},
					{ name: 'Nowhere', latitude: 0, longitude: 0 }
				]
			})
		);

		const geocoder = new Geocoder({ db, logger: silentLogger, provider: 'open-meteo', userAgent: 'test', fetch });
		const places = await geocoder.search('  Berlin ');
		expect(places).toEqual([
			{
				label: 'Berlin, Land Berlin, Germany',
				city: 'Berlin',
				region: 'Land Berlin',
				country: 'DE',
				latE6: 52524370,
				lngE6: 13410530
			}
		]);
		await geocoder.search('berlin');
		expect(fetch).toHaveBeenCalledTimes(1);
	});

	it('supports Nominatim with a user agent', async () => {
		const fetch = vi.fn<typeof globalThis.fetch>(async (_url, init) => {
			expect(new Headers(init?.headers).get('user-agent')).toBe('LocalNetwork test');

			return Response.json([
				{
					display_name: 'Leeds, England, UK',
					lat: '53.8',
					lon: '-1.55',
					address: { town: 'Leeds', state: 'England', country_code: 'gb' }
				}
			]);
		});

		const geocoder = new Geocoder({
			db,
			logger: silentLogger,
			provider: 'nominatim',
			userAgent: 'LocalNetwork test',
			fetch
		});

		expect(await geocoder.search('Leeds')).toEqual([
			{ label: 'Leeds, England, UK', city: 'Leeds', region: 'England', country: 'GB', latE6: 53800000, lngE6: -1550000 }
		]);
	});

	it('returns nothing when disabled, for short queries, or on upstream errors', async () => {
		const off = new Geocoder({ db, logger: silentLogger, provider: 'none', userAgent: 'x' });
		expect(off.enabled).toBe(false);
		expect(await off.search('Berlin')).toEqual([]);

		const failing = new Geocoder({
			db,
			logger: silentLogger,
			provider: 'open-meteo',
			userAgent: 'x',
			fetch: async () => new Response('nope', { status: 500 })
		});

		expect(await failing.search('B')).toEqual([]);
		expect(await failing.search('Boston')).toEqual([]);
	});
});
