import { expect, it } from 'vitest';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

it("names this deployment's syncer service, so space hosts know where to send writes", async () => {
	const app = { config: { serviceDid: 'did:web:localnetwork.computer', publicUrl: 'https://localnetwork.computer' } };

	const response = await callRoute(GET, { app, url: '/.well-known/did.json' });

	expect(response.headers.get('cache-control')).toBe('public, max-age=300');
	expect(await response.json()).toEqual({
		'@context': ['https://www.w3.org/ns/did/v1'],
		id: 'did:web:localnetwork.computer',
		service: [
			{ id: '#localnetwork_syncer', type: 'AtprotoSpaceSyncer', serviceEndpoint: 'https://localnetwork.computer' }
		]
	});
});
