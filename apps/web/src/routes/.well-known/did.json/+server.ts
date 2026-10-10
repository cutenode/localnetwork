import { json } from '@sveltejs/kit';
import { SYNC_SERVICE_FRAGMENT } from '#lib/server/config.ts';
import type { RequestHandler } from './$types';

/**
 * did:web document for this deployment. Space hosts resolve it to deliver
 * `com.atproto.space.notifyWrite` to the registered syncer service.
 */
export const GET: RequestHandler = ({ locals }) => {
	const { config } = locals.app;

	return json(
		{
			'@context': ['https://www.w3.org/ns/did/v1'],
			id: config.serviceDid,
			service: [{ id: `#${SYNC_SERVICE_FRAGMENT}`, type: 'AtprotoSpaceSyncer', serviceEndpoint: config.publicUrl }]
		},
		{ headers: { 'cache-control': 'public, max-age=300' } }
	);
};
