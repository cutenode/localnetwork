import { act, requireViewer, text } from '#lib/server/http.ts';
import { loadPlanner } from '#lib/server/planner-page.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => loadPlanner(locals, requireViewer(locals, url));

export const actions: Actions = {
	unsave: async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();

		return act(async () => {
			await locals.app.planner.unsave(viewer, text(form, 'project', 2048));

			return { success: 'Removed' };
		});
	},
	deleteShow: async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();

		return act(async () => {
			await locals.app.planner.deleteShow(viewer, text(form, 'rkey', 64));

			return { success: 'Show deleted' };
		});
	}
};
