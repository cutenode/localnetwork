import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const { project } = await parent();
	const viewer = requireViewer(locals, url);
	const { app } = locals;
	const shows = unlessOffline(locals.log, () => app.shows.forProject(project));

	const [managers, pendingUpdate] = await Promise.all([
		app.accounts.views(project.managers),
		app.requests.openUpdate(project.uri, viewer.did)
	]);

	return {
		managers: project.managers.map((did) => managers.get(did) ?? { did, handle: null }),
		pendingUpdate,
		// Streamed, so the page shows while the directory finds the shows.
		shows,
		// Which of the viewer's own shows another calendar app changed since, as HappyView has the events.
		edited: shows.then((list) => (list ? app.shows.edited(viewer.did, list) : []))
	};
};
