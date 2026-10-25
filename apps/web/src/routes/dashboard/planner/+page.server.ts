import { UserError } from '#lib/server/core.ts';
import { act, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);

	if (!viewer.spacesCapable) return { available: false as const, saves: [], shows: [], projects: {} };

	try {
		const [saves, shows] = await Promise.all([locals.app.planner.saves(viewer), locals.app.planner.shows(viewer)]);

		const projects = await locals.app.projects.byUris([
			...new Set([...saves.map((s) => s.project), ...shows.flatMap((s) => s.lineup.map((l) => l.project))])
		]);

		return { available: true as const, saves, shows, projects: Object.fromEntries(projects) };
	} catch (error) {
		locals.log.warn({ err: error }, 'planner unavailable');

		return {
			available: false as const,
			saves: [],
			shows: [],
			projects: {},
			problem: error instanceof UserError ? error.message : 'Your PDS did not respond'
		};
	}
};

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
