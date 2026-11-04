import { expect, it } from 'vitest';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

/** What the OAuth client publishes about itself, as a PDS fetches it. */
const metadata = {
	client_id: 'https://localnetwork.test/oauth-client-metadata.json',
	client_name: 'Local Network',
	redirect_uris: ['https://localnetwork.test/oauth/callback'],
	scope: 'atproto',
	dpop_bound_access_tokens: true
};

it('serves the OAuth client metadata, cached for ten minutes', async () => {
	const response = await callRoute(GET, { app: { oauth: { metadata } }, url: '/oauth-client-metadata.json' });

	expect(response.headers.get('cache-control')).toBe('public, max-age=600');
	expect(await response.json()).toEqual(metadata);
});
