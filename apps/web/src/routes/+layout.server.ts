import { requireViewer } from '#lib/server/http.ts';
import type { FlagPreview, Flags, Viewer } from '#lib/types.ts';
import type { LayoutServerLoad } from './$types';

/** `flags` is optional so a page rendered on its own, in a test or a story, reads every flag as off. */
type RootData = { viewer: Viewer | null; pendingRequests: number; flags?: Flags; flagPreview?: FlagPreview };

export const load: LayoutServerLoad = async ({ locals, url }): Promise<RootData> => {
	// Send a session that has to sign in again to the login page before anything tries to write with it.
	if (locals.reauth && url.pathname !== '/login') requireViewer(locals, url);
	const viewer = locals.viewer;
	let pendingRequests = 0;

	if (viewer?.isStaff) pendingRequests = (await locals.app.requests.counts()).pending;

	return { viewer, pendingRequests, flags: locals.flags, flagPreview: locals.flagPreview };
};
