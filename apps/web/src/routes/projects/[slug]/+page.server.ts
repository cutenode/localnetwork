import { error, fail } from '@sveltejs/kit';
import { act, requireViewer } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals }) => {
	const { app, viewer } = locals;
	const project = await app.projects.bySlug(params.slug);

	if (!project) error(404, 'No project with that name');
	const isManager = !!viewer && project.managers.includes(viewer.did);

	if (project.status !== 'active' && !isManager && !viewer?.isStaff) error(404, 'No project with that name');

	const [details, managers, openRequests] = await Promise.all([
		viewer ? app.projects.details(project.uri) : Promise.resolve(null),
		app.accounts.views(project.managers),
		viewer ? app.requests.list({ projectUri: project.uri, requester: viewer.did, status: 'open' }) : Promise.resolve([])
	]);

	let saved: boolean | null = null;

	if (viewer?.spacesCapable) {
		try {
			saved = (await app.planner.saves(viewer)).some((save) => save.project === project.uri);
		} catch (cause) {
			locals.log.debug({ err: cause }, 'could not load planner');
		}
	}

	const linked = (project.profile.mixes ?? []).flatMap((mix) => mix.set ?? []);

	return {
		project,
		// Streamed, so the page shows while the directory looks up the mixes' sets.
		sets: linked.length ? app.sets.linked(linked) : undefined,
		details,
		managers: project.managers.map((did) => managers.get(did) ?? { did, handle: null }),
		isManager,
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
