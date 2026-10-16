import { loadEpk } from '#lib/server/epk-page.ts';
import { act, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params, url }) => {
	const { app, viewer } = locals;
	const { epk, project } = await loadEpk(app.epks, params.did, params.rkey);
	const isOwner = viewer?.did === epk.did;

	if (isOwner) await app.epks.settle(viewer);
	const access = await app.epks.accessState(epk, viewer);
	const canSee = access.kind === 'owner' || access.kind === 'granted';

	const [content, waiting] = await Promise.all([
		canSee ? app.epks.content(epk) : null,
		isOwner
			? app.epks.requests(epk).then((requests) => requests.filter((request) => request.status === 'pending').length)
			: 0
	]);

	return {
		epk,
		project,
		access,
		content,
		waiting,
		saved: url.searchParams.has('saved'),
		// The project's mixes with tracklists, by URL, so the press kit can link each one's tracklist.
		tracklists: Object.fromEntries(
			(project.profile.mixes ?? []).flatMap((mix, index) => (mix.tracklist ? [[mix.url, index + 1]] : []))
		),
		dj: app.dj.activity([epk.owner])
	};
};

export const actions: Actions = {
	request: async ({ request, locals, params, url }) => {
		const viewer = requireViewer(locals, url);
		const { epk } = await loadEpk(locals.app.epks, params.did, params.rkey);
		const form = await request.formData();

		return act(async () => {
			const state = await locals.app.epks.request(viewer, epk, {
				role: text(form, 'role', 32),
				message: text(form, 'message', 1000)
			});

			return { requested: state.kind };
		});
	}
};
