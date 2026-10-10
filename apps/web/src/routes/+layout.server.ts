import { requireViewer } from '#lib/server/http.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ locals, url }) => {
	// Send a session that has to sign in again to the login page before anything tries to write with it.
	if (locals.reauth && url.pathname !== '/login') requireViewer(locals, url);
	const viewer = locals.viewer;
	let pendingRequests = 0;

	if (viewer?.isStaff) pendingRequests = (await locals.app.requests.counts()).pending;

	return { viewer, pendingRequests };
};
