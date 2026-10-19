import { error, fail } from '@sveltejs/kit';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
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

	const linked = (project.profile.mixes ?? []).flatMap((mix) => mix.set ?? []);
	const shows = unlessOffline(locals.log, () => app.shows.forProject(project));

	return {
		...shared,
		// Streamed, so the page shows while the directory looks up the mixes' sets, the shows, and the followers.
		sets: linked.length ? app.sets.linked(linked) : undefined,
		shows,
		// Which of the viewer's own shows another calendar app changed since, read from their repo.
		edited:
			shared.isManager && viewer ? shows.then((list) => (list ? app.shows.edited(viewer.did, list) : [])) : undefined,
		follow:
			project.status === 'active'
				? unlessOffline(locals.log, () => app.follows.state(project, viewer?.did))
				: undefined,
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
	},
	follow: async ({ params, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		// A thrown 404, not a failure: as a failure, TypeScript would fold `act`'s `reauth` out of the result type.
		if (!project) error(404, 'Project not found');

		return act(async () => {
			await locals.app.follows.follow(viewer, project);

			return { followed: true };
		});
	},
	unfollow: async ({ params, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		// A thrown 404, not a failure: as a failure, TypeScript would fold `act`'s `reauth` out of the result type.
		if (!project) error(404, 'Project not found');

		return act(async () => {
			await locals.app.follows.unfollow(viewer, project);

			return { followed: false };
		});
	}
};
