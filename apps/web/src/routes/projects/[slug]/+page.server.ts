import { fail } from '@sveltejs/kit';
import { act, requireViewer } from '#lib/server/http.ts';
import { loadProjectPage } from '#lib/server/project-page.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals }) => {
	const { app, viewer } = locals;
	const shared = await loadProjectPage(locals, params.slug);
	const { project } = shared;

	const openRequests = viewer
		? await app.requests.list({ projectUri: project.uri, requester: viewer.did, status: 'open' })
		: [];

	let saved: boolean | null = null;

	if (viewer?.spacesCapable) {
		try {
			saved = (await app.planner.saves(viewer)).some((save) => save.project === project.uri);
		} catch (cause) {
			locals.log.debug({ err: cause }, 'could not load planner');
		}
	}

	return {
		...shared,
		saved,
		pendingClaim: openRequests.find((request) => request.kind === 'claim') ?? null,
		pendingUpdate: openRequests.find((request) => request.kind === 'update') ?? null
	};
};

export const actions: Actions = {
	save: async ({ params, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) return fail(404, { error: 'Project not found' });

		return act(async () => {
			await locals.app.planner.save(viewer, project.uri);

			return { success: 'Saved to your planner' };
		});
	},
	unsave: async ({ params, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) return fail(404, { error: 'Project not found' });

		return act(async () => {
			await locals.app.planner.unsave(viewer, project.uri);

			return { success: 'Removed from your planner' };
		});
	}
};
