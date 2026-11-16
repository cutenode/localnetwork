import { soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const requests = locals.app.requests.list({ requester: viewer.did, limit: 50 });

	requests.catch((error) => locals.log.warn('could not list what someone asked staff for', { err: error }));

	return { requests: (await soon(requests)).value };
};
