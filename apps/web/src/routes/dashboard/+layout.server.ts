import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, url }) => {
	const viewer = requireViewer(locals, url);

	// Streamed, so the dashboard shows while the directory finds them; null when it's offline.
	return { projects: unlessOffline(locals.log, () => locals.app.projects.managedBy(viewer.did)) };
};
