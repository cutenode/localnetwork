export type Place = {
	label: string;
	city: string;
	region?: string;
	country: string;
	latE6: number;
	lngE6: number;
};

/** The body `/api/places` answers with. */
export type PlacesResponse = { places: Place[] };

/** City search through `/api/places`. An error response, like the rate limit's 429, finds nothing. */
export async function searchPlaces(query: string): Promise<Place[]> {
	const response = await fetch(`/api/places?q=${encodeURIComponent(query)}`);

	if (!response.ok) return [];

	// SAFETY: /api/places is this app's own endpoint (src/routes/api/places/+server.ts), and every body it sends `satisfies PlacesResponse`.
	return ((await response.json()) as PlacesResponse).places;
}
