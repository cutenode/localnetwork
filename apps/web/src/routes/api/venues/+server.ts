import { json } from '@sveltejs/kit';
import { z } from 'zod';
import { UserError } from '#lib/server/core.ts';
import { clientIp, limit, requireFlag } from '#lib/server/http.ts';
import { venueOption } from '#lib/server/services/venues.ts';
import type { AddVenueResponse, VenuesResponse } from '#lib/venues.ts';
import type { RequestHandler } from './$types';

/** A venue's name and the city picked for it from the city search. */
const addBody = z.object({
	name: z.string().max(640),
	city: z.object({
		city: z.string().min(1).max(1000),
		region: z.string().max(1000).optional(),
		country: z.string().length(2),
		latE6: z.number().int().min(-90_000_000).max(90_000_000),
		lngE6: z.number().int().min(-180_000_000).max(180_000_000)
	})
});

/** Venues by name, for the venue picker. Events are behind spacePages, so the picker is too. */
export const GET: RequestHandler = async (event) => {
	const { app } = event.locals;

	requireFlag(event.locals, 'spacePages');

	if (!app.limits.search.take(`venues:${clientIp(event)}`))
		return json({ venues: [] } satisfies VenuesResponse, { status: 429 });
	const q = event.url.searchParams.get('q') ?? '';

	return json({ venues: await app.venues.search(q) } satisfies VenuesResponse, {
		headers: { 'cache-control': 'private, max-age=60' }
	});
};

/** Adds a venue by its name and city, or answers with the one already there. */
export const POST: RequestHandler = async (event) => {
	const { app, viewer, log } = event.locals;

	requireFlag(event.locals, 'spacePages');

	if (!viewer) return json({ error: 'Sign in to add a venue' } satisfies AddVenueResponse, { status: 401 });
	const body = addBody.safeParse(await event.request.json().catch(() => null));

	if (!body.success)
		return json({ error: 'Pick the city from the search' } satisfies AddVenueResponse, { status: 400 });
	const { city } = body.data;

	try {
		const name = limit('The name', body.data.name.trim(), 64);

		if (!name) return json({ error: "Enter the venue's name" } satisfies AddVenueResponse, { status: 400 });

		const venue = await app.venues.add(viewer, {
			name,
			address: {
				country: city.country.toUpperCase(),
				locality: city.city,
				...(city.region && { region: city.region })
			},
			geo: { latitude: String(city.latE6 / 1e6), longitude: String(city.lngE6 / 1e6) }
		});

		return json({ venue: venueOption(venue) } satisfies AddVenueResponse);
	} catch (error) {
		if (!(error instanceof UserError)) throw error;
		log.info('refused to add a venue', { reason: error.message });

		return json({ error: error.message } satisfies AddVenueResponse, { status: error.status });
	}
};
