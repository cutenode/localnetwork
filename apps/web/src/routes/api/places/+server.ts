import { json } from '@sveltejs/kit';
import type { PlacesResponse } from '#lib/places.ts';
import { clientIp } from '#lib/server/http.ts';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async (event) => {
	const { app } = event.locals;

	if (!app.limits.search.take(`places:${clientIp(event)}`))
		return json({ places: [] } satisfies PlacesResponse, { status: 429 });
	const q = event.url.searchParams.get('q') ?? '';

	return json({ places: await app.geocoder.search(q) } satisfies PlacesResponse, {
		headers: { 'cache-control': 'private, max-age=300' }
	});
};
