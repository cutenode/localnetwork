import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);

	return { requests: await locals.app.requests.list({ requester: viewer.did, limit: 50 }) };
};
