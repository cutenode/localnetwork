import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	// Press kits this account made before the root held them move to the root now, without holding the page up; they
	// stay listed and readable where they are until then.
	void locals.app.epks
		.migrateLegacy(viewer)
		.catch((error) => locals.log.warn({ err: error }, 'could not move legacy press kits'));

	// Streamed, so the dashboard shows while the directory finds them; null when it's offline.
	return { projects: unlessOffline(locals.log, () => locals.app.projects.managedBy(viewer.did)) };
};
