import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const { project } = await parent();

	return {
		viewerDid: requireViewer(locals, url).did,
		// Streamed, so the page shows while the press kits and drafts are read.
		dashboard: locals.app.epks.dashboard(project),
		deleted: url.searchParams.has('deleted')
	};
};
