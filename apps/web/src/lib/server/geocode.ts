import { z } from 'zod';
import { toE6 } from '#lib/geo.ts';
import type { Place } from '#lib/places.ts';
import { type Db, nowIso, parseJson } from './db';
import type { Logger } from './logger';

type Provider = 'open-meteo' | 'nominatim' | 'none';

const openMeteoResponse = z.object({
	results: z
		.array(
			z.object({
				name: z.string(),
				admin1: z.string().optional(),
				country_code: z.string().optional(),
				country: z.string().optional(),
				latitude: z.number(),
				longitude: z.number()
			})
		)
		.optional()
});

const nominatimResponse = z.array(
	z.object({
		display_name: z.string(),
		lat: z.string(),
		lon: z.string(),
		address: z
			.object({
				city: z.string().optional(),
				town: z.string().optional(),
				village: z.string().optional(),
				state: z.string().optional(),
				country_code: z.string().optional()
			})
			.optional()
	})
);

/**
 * City search for location fields. Results are cached in SQLite for 30 days.
 * With GEOCODER=none, locations are entered by hand.
 */
export class Geocoder {
	constructor(
		private readonly deps: { db: Db; logger: Logger; provider: Provider; userAgent: string; fetch?: typeof fetch }
	) {}

	get enabled(): boolean {
		return this.deps.provider !== 'none';
	}

	async search(query: string): Promise<Place[]> {
		const q = query.trim().replace(/\s+/g, ' ').slice(0, 100);

		if (q.length < 2 || !this.enabled) return [];
		const key = `${this.deps.provider}:${q.toLowerCase()}`;

		const cached = await this.deps.db
			.selectFrom('geocodeCache')
			.selectAll()
			.where('query', '=', key)
			.executeTakeFirst();

		if (cached && Date.now() - Date.parse(cached.createdAt) < 30 * 86_400_000)
			return parseJson<Place[]>(cached.results, []);
		let results: Place[] = [];

		try {
			results = this.deps.provider === 'nominatim' ? await this.nominatim(q) : await this.openMeteo(q);
		} catch (error) {
			this.deps.logger.warn({ err: error, query: q }, 'geocoding failed');

			return [];
		}

		const json = JSON.stringify(results);
		await this.deps.db
			.insertInto('geocodeCache')
			.values({ query: key, results: json, createdAt: nowIso() })
			.onConflict((oc) => oc.column('query').doUpdateSet({ results: json, createdAt: nowIso() }))
			.execute();

		return results;
	}

	private async openMeteo(q: string): Promise<Place[]> {
		const url = new URL('https://geocoding-api.open-meteo.com/v1/search');
		url.searchParams.set('name', q);
		url.searchParams.set('count', '8');
		url.searchParams.set('format', 'json');
		const response = await (this.deps.fetch ?? fetch)(url, { signal: AbortSignal.timeout(5_000) });

		if (!response.ok) throw new Error(`open-meteo ${response.status}`);

		const body = openMeteoResponse.parse(await response.json());

		return (body.results ?? [])
			.filter((item) => item.country_code)
			.map((item) => ({
				label: [item.name, item.admin1, item.country].filter(Boolean).join(', '),
				city: item.name,
				region: item.admin1,
				country: item.country_code!.toUpperCase(),
				latE6: toE6(item.latitude),
				lngE6: toE6(item.longitude)
			}));
	}

	private async nominatim(q: string): Promise<Place[]> {
		const url = new URL('https://nominatim.openstreetmap.org/search');
		url.searchParams.set('q', q);
		url.searchParams.set('format', 'jsonv2');
		url.searchParams.set('addressdetails', '1');
		url.searchParams.set('limit', '8');
		url.searchParams.set('featureType', 'city');

		const response = await (this.deps.fetch ?? fetch)(url, {
			headers: { 'user-agent': this.deps.userAgent },
			signal: AbortSignal.timeout(5_000)
		});

		if (!response.ok) throw new Error(`nominatim ${response.status}`);

		const body = nominatimResponse.parse(await response.json());

		return body
			.filter((item) => item.address?.country_code)
			.map((item) => {
				const city =
					item.address?.city ?? item.address?.town ?? item.address?.village ?? item.display_name.split(',')[0];

				return {
					label: item.display_name,
					city,
					region: item.address?.state,
					country: item.address!.country_code!.toUpperCase(),
					latE6: toE6(Number(item.lat)),
					lngE6: toE6(Number(item.lon))
				};
			});
	}
}
