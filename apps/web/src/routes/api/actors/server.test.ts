import { expect, it } from 'vitest';
import type { ActorSuggestion } from '#lib/actors.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const ACCOUNTS: ActorSuggestion[] = [
	{ did: 'did:plc:neon', handle: 'neontide.test', displayName: 'Neon Tide' },
	{ did: 'did:plc:nightshift', handle: 'nightshift.test' }
];

/** Actor search over a fixed list of accounts, by handle or name prefix, allowing `searches` a burst per address. */
const app = (searches = 60) => ({
	limits: { search: new RateLimiter(searches, 2) },
	actorSearch: {
		search: async (query: string) =>
			ACCOUNTS.filter(
				(actor) => query && [actor.handle, actor.displayName ?? ''].some((name) => name.toLowerCase().startsWith(query))
			)
	}
});

it('suggests accounts whose handle or name starts with the query, cached privately', async () => {
	const response = await callRoute(GET, { app: app(), url: '/api/actors?q=neon' });

	expect(response.headers.get('cache-control')).toBe('private, max-age=300');
	expect(await response.json()).toEqual({ actors: [ACCOUNTS[0]] });
});

it('finds nothing without a query', async () => {
	const response = await callRoute(GET, { app: app(), url: '/api/actors' });

	expect(await response.json()).toEqual({ actors: [] });
});

it('finds nothing, with a 429, once an address searches too often', async () => {
	const response = await callRoute(GET, { app: app(0), url: '/api/actors?q=neon' });

	expect(response.status).toBe(429);
	expect(await response.json()).toEqual({ actors: [] });
});
