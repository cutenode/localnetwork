import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

/** did:web document for this deployment, the audience of the service auth it accepts. */
export const GET: RequestHandler = ({ locals }) => {
	const { config } = locals.app;

	return json(
		{
			'@context': ['https://www.w3.org/ns/did/v1'],
			id: config.serviceDid
		},
		{ headers: { 'cache-control': 'public, max-age=300' } }
	);
};
