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

	// The layout's requireManager let only this project's managers through.
	const own = managers.get(viewer.did);

	return {
		managers: project.managers.map((did) => managers.get(did)),
		// Streamed, for the press kit and stats summaries; a lookup that fails leaves its summary out.
		kits: app.epks.dashboard(project).catch((error) => {
			locals.log.warn('could not read press kits for the project summary', { err: error });

			return null;
		}),
		stats: app.dj.fresh([own]) ?? app.dj.activity([own]),
		pendingUpdate,
		// Streamed, so the page shows while the directory finds the shows.
		shows,
		// Which of the viewer's own shows another calendar app changed since, as HappyView has the events.
		edited: shows.then((list) => (list ? app.shows.edited(viewer.did, list) : []))
	};
};
