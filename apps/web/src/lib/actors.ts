/** An account the sign-in typeahead suggests. */
export type ActorSuggestion = { did: string; handle: string; displayName?: string; avatar?: string };

/** The body `/api/actors` answers with. */
export type ActorsResponse = { actors: ActorSuggestion[] };

/** Accounts whose handle or name starts with `query`, through `/api/actors`. An error response finds nothing. */
export async function searchActors(query: string, signal?: AbortSignal): Promise<ActorSuggestion[]> {
	const response = await fetch(`/api/actors?q=${encodeURIComponent(query)}`, { signal });

	if (!response.ok) return [];

	// SAFETY: /api/actors is this app's own endpoint (src/routes/api/actors/+server.ts), and every body it sends `satisfies ActorsResponse`.
	return ((await response.json()) as ActorsResponse).actors;
}
