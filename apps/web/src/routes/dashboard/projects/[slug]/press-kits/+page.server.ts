import { requireViewer, act, text } from '#lib/server/http.ts';
import { requireManager } from '#lib/server/manage.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const { project } = await parent();

	return {
		viewerDid: requireViewer(locals, url).did,
		// Streamed, so the page shows while the press kits and drafts are read.
		dashboard: locals.app.epks.dashboard(project),
		views: locals.app.epkViews.list(project),
		retentionDays: locals.app.epkViews.retentionDays,
		deleted: url.searchParams.has('deleted')
	};
};

export const actions: Actions = {
	forgetView: async ({ request, locals, url, params }) => {
		const { viewer, project } = await requireManager(locals, url, params.slug);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epkViews.remove(viewer, project, text(form, 'view', 64));

			return { success: 'View deleted.' };
		});
	}
};
